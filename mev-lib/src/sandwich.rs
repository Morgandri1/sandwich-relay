use solana_sdk::{
    instruction::CompiledInstruction, message::VersionedMessage, pubkey::Pubkey, signature::{Keypair, Signature, Signer}, transaction::VersionedTransaction
};
use std::ops::{Deref, DerefMut};
use std::fmt::{Debug, Formatter, Result as FmtResult};
use solana_perf::packet::Packet;
use crate::{programs::mev::MEV_PROGRAM_ID, result::{MevError, MevResult}};
use crate::tx::build_tx_sandwich;

/// Priority values for different types of transactions within a sandwich
pub const PRIORITY_FRONTRUN: u8 = 1;
pub const PRIORITY_ORIGINAL: u8 = 2;
pub const PRIORITY_BACKRUN: u8 = 3;

/// A wrapper for VersionedTransaction with an additional priority field
#[derive(Clone)]
pub struct PrioritizedTx {
    /// The inner transaction
    pub transaction: VersionedTransaction,
    /// The priority flag (1=frontrun, 2=original, 3=backrun)
    pub priority: u8,
}

impl PrioritizedTx {
    /// Create a new prioritized transaction
    pub fn new(transaction: VersionedTransaction, priority: u8) -> Self {
        Self {
            transaction,
            priority,
        }
    }
    
    /// Get the signature of this transaction
    pub fn signature(&self) -> Option<&Signature> {
        self.transaction.signatures.first()
    }
}

impl Deref for PrioritizedTx {
    type Target = VersionedTransaction;
    
    fn deref(&self) -> &Self::Target {
        &self.transaction
    }
}

impl DerefMut for PrioritizedTx {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.transaction
    }
}

impl Debug for PrioritizedTx {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        let priority_str = match self.priority {
            PRIORITY_FRONTRUN => "FRONTRUN",
            PRIORITY_ORIGINAL => "ORIGINAL",
            PRIORITY_BACKRUN => "BACKRUN",
            _ => "UNKNOWN",
        };
        
        write!(f, "PrioritizedTx {{ priority: {} ({}), ", self.priority, priority_str)?;
        
        if let Some(sig) = self.signature() {
            write!(f, "sig: {}, ", sig)?;
        } else {
            write!(f, "sig: None, ")?;
        }
        
        write!(f, "transaction: {:?} }}", self.transaction)
    }
}

/// A group of related sandwich transactions
#[derive(Clone)]
pub struct SandwichGroup {
    /// The frontrun transaction
    pub frontrun: Option<PrioritizedTx>,
    /// The original transaction
    pub original: PrioritizedTx,
    /// The backrun transaction
    pub backrun: Option<PrioritizedTx>,
}

impl SandwichGroup {
    /// Create a new sandwich group from an original transaction
    pub fn new(original_tx: VersionedTransaction) -> Self {
        Self {
            frontrun: None,
            original: PrioritizedTx::new(original_tx, PRIORITY_ORIGINAL),
            backrun: None,
        }
    }
    
    /// Create sandwich transactions around the original
    pub fn create_sandwich(&mut self, keypair: &Keypair) -> MevResult<()> {
        // Build the sandwich transactions
        let sandwich_tx_messages = build_tx_sandwich(&self.original.transaction, keypair)?;
        
        // Ensure we got the expected number of transactions (3 for a full sandwich)
        if sandwich_tx_messages.len() != 3 {
            return Err(MevError::UnknownError);
        }
        
        // Extract the messages
        let frontrun_msg = &sandwich_tx_messages[0];
        // The original message is at index 1, but we already have it
        let backrun_msg = &sandwich_tx_messages[2];
        
        // Create and sign the frontrun transaction
        let mut frontrun_tx = VersionedTransaction {
            signatures: vec![],
            message: frontrun_msg.clone(),
        };
        let frontrun_signature = keypair.sign_message(&frontrun_tx.message.serialize());
        frontrun_tx.signatures = vec![frontrun_signature];
        
        // Create the backrun transaction
        let mut backrun_tx = VersionedTransaction {
            signatures: vec![],
            message: backrun_msg.clone(),
        };
        let backrun_signature = keypair.sign_message(&backrun_tx.message.serialize());
        backrun_tx.signatures = vec![backrun_signature];
        
        // Store the transactions with their priorities
        self.frontrun = Some(PrioritizedTx::new(
            frontrun_tx,
            PRIORITY_FRONTRUN
        ));
        
        self.backrun = Some(PrioritizedTx::new(
            backrun_tx,
            PRIORITY_BACKRUN
        ));
        
        Ok(())
    }
    
    /// Convert this sandwich group to a vector of packets in the correct order:
    /// [frontrun, original, backrun]
    pub fn to_packets(&self) -> MevResult<Vec<(Packet, Signature)>> {
        let mut packets = Vec::with_capacity(3);
        
        // Create packets in strict order: frontrun, original, backrun
        
        // Add frontrun packet if it exists
        if let Some(frontrun) = &self.frontrun {
            if let Some(signature) = frontrun.signature() {
                // Serialize the transaction
                let tx_data = bincode::serialize(&frontrun.transaction)
                    .map_err(|_| MevError::FailedToSerialize)?;
                
                // Create the packet
                match Packet::from_data(None, &tx_data) {
                    Ok(packet) => {
                        packets.push((packet, *signature));
                    },
                    Err(_) => return Err(MevError::FailedToSerialize),
                }
            }
        }
        
        // Add original packet (must exist)
        if let Some(signature) = self.original.signature() {
            // Serialize the transaction
            let tx_data = bincode::serialize(&self.original.transaction)
                .map_err(|_| MevError::FailedToSerialize)?;
            
            // Create the packet
            match Packet::from_data(None, &tx_data) {
                Ok(packet) => {
                    packets.push((packet, *signature));
                },
                Err(_) => return Err(MevError::FailedToSerialize),
            }
        } else {
            return Err(MevError::FailedToDeserialize);
        }
        
        // Add backrun packet if it exists
        if let Some(backrun) = &self.backrun {
            if let Some(signature) = backrun.signature() {
                // Serialize the transaction
                let tx_data = bincode::serialize(&backrun.transaction)
                    .map_err(|_| MevError::FailedToSerialize)?;
                
                // Create the packet
                match Packet::from_data(None, &tx_data) {
                    Ok(packet) => {
                        packets.push((packet, *signature));
                    },
                    Err(_) => return Err(MevError::FailedToSerialize),
                }
            }
        }
        
        Ok(packets)
    }
    
    /// Get all transactions in this group as a vector in the correct order:
    /// [frontrun, original, backrun]
    #[allow(dead_code)]
    pub fn get_all_transactions(&self) -> Vec<PrioritizedTx> {
        let mut result = Vec::with_capacity(3);
        
        if let Some(frontrun) = &self.frontrun {
            result.push(frontrun.clone());
        }
        
        result.push(self.original.clone());
        
        if let Some(backrun) = &self.backrun {
            result.push(backrun.clone());
        }
        
        result
    }
}

fn filter_instructions(message: &VersionedMessage) -> MevResult<CompiledInstruction> {
    let ix: Vec<&CompiledInstruction> = message
        .instructions()
        .iter()
        .filter(|ix| ix.program_id(message.static_account_keys()) == &MEV_PROGRAM_ID)
        .collect();
    if ix.len() == 1 {
        return Ok(ix[0].clone())
    } else {
        return Err(MevError::FailedToDeserialize)
    }
}

/// Verify that packets are in the correct order for sandwich execution
/// This checks that any front/original/backrun packets are in the correct sequence
pub fn verify_sandwich_preflight(packets: &[Packet]) -> MevResult<bool> {
    if packets.len() < 3 {
        return Ok(true); // Not enough packets for a sandwich
    }
    
    let vtxs: Vec<VersionedTransaction> = packets
        .iter()
        .map(|p| p.deserialize_slice::<VersionedTransaction, _>(..))
        .filter(|r| r.is_ok())
        .map(|r| r.unwrap())
        .collect();
    
    if vtxs.len() != packets.len() {
        eprintln!("{:?}", vtxs.len());
        return Err(MevError::FailedToDeserialize);
    }
    
    let run_ix: Vec<CompiledInstruction> = vtxs
        .iter()
        .filter(|vtx| vtx.message.static_account_keys().contains(&MEV_PROGRAM_ID)) // filter out the original swap
        .map(|fvtx| match filter_instructions(&fvtx.message) { // filter front-run tx messages down to specific ix
            Ok(ix) => ix,
            Err(_) => panic!()
        })
        .collect();
    
    if run_ix.len() < 2 {
        return Ok(false)
    }
    
    // frontrun will always contain more ix data than backrun
    if run_ix[0].data.len() > run_ix[1].data.len() {
        return Ok(true)
    } else {
        return Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    // Helper to create a test transaction
    fn create_test_transaction() -> VersionedTransaction {
        let keypair = Keypair::new();
        
        // Create a simple transaction for testing
        let tx = solana_sdk::transaction::Transaction::new_unsigned(
            solana_sdk::message::Message::new_with_blockhash(
                &[],
                Some(&keypair.pubkey()),
                &solana_sdk::hash::Hash::default(),
            )
        );
        
        VersionedTransaction::from(tx)
    }
    
    #[test]
    fn test_prioritized_tx() {
        // Create a test transaction
        let tx = create_test_transaction();
        
        // Create prioritized transaction
        let prioritized = PrioritizedTx::new(tx.clone(), PRIORITY_ORIGINAL);
        
        // Check priority
        assert_eq!(prioritized.priority, PRIORITY_ORIGINAL);
    }
    
    #[test]
    fn test_sandwich_group_creation() {
        // Create a test transaction
        let tx = create_test_transaction();
        
        // Create sandwich group
        let group = SandwichGroup::new(tx.clone());
        
        // Verify original is set
        assert_eq!(group.original.priority, PRIORITY_ORIGINAL);
        assert!(group.frontrun.is_none());
        assert!(group.backrun.is_none());
        
        // Check get_all_transactions
        let txs = group.get_all_transactions();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].priority, PRIORITY_ORIGINAL);
    }
    
    #[test]
    fn test_verify_sandwich_preflight() {
        use solana_sdk::{
            message::Message,
            transaction::Transaction,
            instruction::{Instruction, AccountMeta},
            system_program,
            hash::Hash,
            pubkey::Pubkey,
        };
        
        // Create keypairs for signers
        let original_signer = Keypair::new();
        let sandwich_signer = Keypair::new();
        
        // Create frontrun transaction (signed by sandwich_signer)
        let frontrun_ix = Instruction {
            program_id: MEV_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(sandwich_signer.pubkey(), true),
                AccountMeta::new_readonly(system_program::id(), false),
                AccountMeta::new(MEV_PROGRAM_ID, false)
            ],
            data: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24],
        };
        let frontrun_message = Message::new(&[frontrun_ix], Some(&sandwich_signer.pubkey()));
        let frontrun_tx = Transaction::new(&[&sandwich_signer], frontrun_message, Hash::default());
        let frontrun_vtx = VersionedTransaction::from(frontrun_tx);
        
        // Create original transaction (signed by original_signer)
        let original_ix = Instruction {
            program_id: Pubkey::new_unique(),
            accounts: vec![
                AccountMeta::new(original_signer.pubkey(), true),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data: vec![5, 6, 7, 8],
        };
        let original_message = Message::new(&[original_ix], Some(&original_signer.pubkey()));
        let original_tx = Transaction::new(&[&original_signer], original_message, Hash::default());
        let original_vtx = VersionedTransaction::from(original_tx);
        
        // Create backrun transaction (signed by sandwich_signer)
        let backrun_ix = Instruction {
            program_id: MEV_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(sandwich_signer.pubkey(), true),
                AccountMeta::new_readonly(system_program::id(), false),
                AccountMeta::new(MEV_PROGRAM_ID, false)
            ],
            data: vec![9, 10, 11, 12, 13, 14],
        };
        let backrun_message = Message::new(&[backrun_ix], Some(&sandwich_signer.pubkey()));
        let backrun_tx = Transaction::new(&[&sandwich_signer], backrun_message, Hash::default());
        let backrun_vtx = VersionedTransaction::from(backrun_tx);
        
        // Create packets
        let frontrun_data = bincode::serialize(&frontrun_vtx).unwrap();
        let original_data = bincode::serialize(&original_vtx).unwrap();
        let backrun_data = bincode::serialize(&backrun_vtx).unwrap();
        
        let frontrun_packet = Packet::from_data(None, &frontrun_data).unwrap();
        let original_packet = Packet::from_data(None, &original_data).unwrap();
        let backrun_packet = Packet::from_data(None, &backrun_data).unwrap();
        
        let packets_correct = vec![
            frontrun_packet.clone(), 
            original_packet.clone(), 
            backrun_packet.clone()
        ];
        assert!(verify_sandwich_preflight(&packets_correct).unwrap());
        
        let packets_incorrect = vec![
            backrun_packet.clone(), 
            original_packet.clone(),
            frontrun_packet.clone()
        ];
        assert!(!verify_sandwich_preflight(&packets_incorrect).unwrap());
        
        // Test with insufficient packets - should pass since we can't verify
        let packets_incomplete = vec![frontrun_packet.clone(), original_packet.clone()];
        assert!(verify_sandwich_preflight(&packets_incomplete).unwrap());
        
        // Test with unrelated packets
        let unrelated_signer = Keypair::new();
        let unrelated_ix = Instruction {
            program_id: Pubkey::new_unique(),
            accounts: vec![
                AccountMeta::new(unrelated_signer.pubkey(), true),
            ],
            data: vec![13, 14, 15],
        };
        let unrelated_message = Message::new(&[unrelated_ix], Some(&unrelated_signer.pubkey()));
        let unrelated_tx = Transaction::new(&[&unrelated_signer], unrelated_message, Hash::default());
        let unrelated_vtx = VersionedTransaction::from(unrelated_tx);
        let unrelated_data = bincode::serialize(&unrelated_vtx).unwrap();
        let unrelated_packet = Packet::from_data(None, &unrelated_data).unwrap();
        
        // Test with mixed packets
        let packets_mixed = vec![
            unrelated_packet.clone(),
            frontrun_packet.clone(),
            original_packet.clone(),
            backrun_packet.clone(),
            unrelated_packet.clone()
        ];
        
        assert!(verify_sandwich_preflight(&packets_mixed).unwrap());
    }
}