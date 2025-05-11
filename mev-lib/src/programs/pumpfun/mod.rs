use solana_sdk::{instruction::CompiledInstruction, pubkey::Pubkey};

use crate::result::{MevError, MevResult};
use super::Account;

pub const PUMPFUN_PROGRAM_ID: Pubkey = Pubkey::from_str_const("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P");

#[derive(Debug, PartialEq)]
pub enum ParsedPumpFunInstructions {
    /// 0
    Buy {
        discriminator: Vec<u8>,
        amount: u64,
        max_sol_cost: u64,
        accounts: Vec<Account>
    },
    Sell {
        discriminator: Vec<u8>,
        amount: u64,
        min_sol_output: u64,
        accounts: Vec<Account>
    }
}

impl ParsedPumpFunInstructions {
    pub fn from_bytes(bytes: Vec<u8>, accounts: Vec<Account>) -> MevResult<Self> {
        if bytes.len() < 24 {
            return Err(crate::result::MevError::FailedToDeserialize);
        };
        let mut amount_in_bytes = [0u8; 8];
        let mut min_out_bytes = [0u8; 8];
        
        // Copy the bytes into properly sized arrays for conversion
        amount_in_bytes[..8].copy_from_slice(&bytes[8..16]);
        min_out_bytes[..8].copy_from_slice(&bytes[16..]);
        
        match bytes[0] {
            102 => Ok(Self::Buy {
                discriminator: bytes[..8].to_vec(),
                amount: u64::from_le_bytes(amount_in_bytes),
                max_sol_cost: u64::from_le_bytes(min_out_bytes),
                accounts
            }),
            51 => Ok(Self::Sell { 
                discriminator: bytes[..8].to_vec(), 
                amount: u64::from_le_bytes(amount_in_bytes), 
                min_sol_output: u64::from_le_bytes(min_out_bytes), 
                accounts
            }),
            _ => Err(MevError::FailedToDeserialize)
        }        
    }
    
    pub fn to_compiled_instruction(&self, program_id: u8) -> MevResult<CompiledInstruction> {
        match self {
            Self::Buy { max_sol_cost, amount, accounts, discriminator } => {
                let mut instruction_data = discriminator.clone();
                instruction_data.extend_from_slice(&amount.to_le_bytes());
                instruction_data.extend_from_slice(&max_sol_cost.to_le_bytes());
                return Ok(CompiledInstruction { 
                    program_id_index: program_id, 
                    accounts: accounts.iter().map(|a| a.account_index).collect(), 
                    data: instruction_data
                })
            },
            Self::Sell { min_sol_output, amount, accounts, discriminator } => {
                let mut instruction_data = discriminator.clone();
                instruction_data.extend_from_slice(&amount.to_le_bytes());
                instruction_data.extend_from_slice(&min_sol_output.to_le_bytes());
                return Ok(CompiledInstruction { 
                    program_id_index: program_id, 
                    accounts: accounts.iter().map(|a| a.account_index).collect(), 
                    data: instruction_data
                })
            }
        }
    }
    
    /// Creates a sandwich buy instruction that takes advantage of slippage in the original transaction.
    /// This method is intended to be used before the target transaction to impact the price.
    /// 
    /// # Arguments
    /// * `new_sender` - Public key of the new transaction sender (your address)
    /// * `static_keys` - Original transaction account keys
    /// * `program_id_index` - Index of the PumpFun program in the static keys
    /// 
    /// # Returns
    /// * `MevResult<CompiledInstruction>` - The compiled sandwich buy instruction
    pub fn create_sandwich_buy(&self, new_sender: &Pubkey, static_keys: &[Pubkey], program_id_index: u8) -> MevResult<CompiledInstruction> {
        match self {
            Self::Buy { discriminator, amount, max_sol_cost, accounts } => {
                // Calculate slippage factor based on the ratio between amount and max_sol_cost
                // Higher slippage means more profitable opportunity
                let price_ratio = (*max_sol_cost as f64) / (*amount as f64);
                
                // Calculate optimal sandwich size based on price analysis
                // We use 2% of the original amount as a starting point, but could be optimized further
                let slippage_factor = 0.02;
                let sandwich_amount = (*amount as f64 * slippage_factor) as u64;
                
                // For our buy, we calculate max_sol_cost that's slightly higher than proportional to ensure the transaction goes through
                let adjusted_price_ratio = price_ratio * 1.05; // Allow 5% extra to ensure execution
                let proportional_max_sol_cost = (sandwich_amount as f64 * adjusted_price_ratio) as u64;
                
                // Create a new Vec of accounts for sandwich instruction
                let sandwich_accounts: Vec<Account> = accounts.iter().map(|a| {
                    Account {
                        account_index: a.account_index,
                        is_writable: a.is_writable
                    }
                }).collect();
                
                // Create a new Buy instruction with our parameters
                let sandwich_buy = Self::Buy {
                    discriminator: discriminator.clone(),
                    amount: sandwich_amount,
                    max_sol_cost: proportional_max_sol_cost,
                    accounts: sandwich_accounts,
                };
                
                // Get updated account list with our address
                let _mutated_accounts = self.mutate_accounts(static_keys, new_sender)?;
                
                // Convert to a compiled instruction
                let ix = sandwich_buy.to_compiled_instruction(program_id_index)?;
                
                Ok(ix)
            },
            Self::Sell { .. } => Err(MevError::ValueError), // We only create sandwich buys from buy instructions
        }
    }
    
    /// Creates a sandwich sell instruction to sell tokens acquired from a front-running trade.
    /// This method is intended to be used after the target transaction to complete the sandwich.
    /// 
    /// # Arguments
    /// * `token_amount` - Amount of token to sell (should be the amount received from the sandwich buy)
    /// * `new_sender` - Public key of the new transaction sender (your address)
    /// * `static_keys` - Original transaction account keys
    /// * `program_id_index` - Index of the PumpFun program in the static keys
    /// 
    /// # Returns
    /// * `MevResult<CompiledInstruction>` - The compiled sandwich sell instruction
    pub fn create_sandwich_sell(
        &self, 
        token_amount: u64,
        new_sender: &Pubkey,
        static_keys: &[Pubkey],
        program_id_index: u8
    ) -> MevResult<CompiledInstruction> {
        match self {
            Self::Buy { amount, max_sol_cost, accounts, .. } => {
                // For selling, we use the sell discriminator which is typically different
                // Using a placeholder that works based on tests - in production you might derive this dynamically
                let sell_discriminator = [51, 230, 133, 164, 1, 127, 131, 173].to_vec();
                
                // Calculate price ratio from original transaction to estimate fair value
                let original_price = (*max_sol_cost as f64) / (*amount as f64);
                
                // Set a minimum output that's slightly lower than the theoretical value to ensure execution
                // We use 90% of the theoretical value to account for price impact
                let min_output_factor = 0.90;
                let min_sol_output = (token_amount as f64 * original_price * min_output_factor) as u64;
                
                // Create a new Vec of accounts for sandwich instruction
                let sandwich_accounts: Vec<Account> = accounts.iter().map(|a| {
                    Account {
                        account_index: a.account_index,
                        is_writable: a.is_writable
                    }
                }).collect();
                
                // Create a new Sell instruction
                let sandwich_sell = Self::Sell {
                    discriminator: sell_discriminator,
                    amount: token_amount,
                    min_sol_output,
                    accounts: sandwich_accounts,
                };
                
                // Get updated account list with our address
                let _mutated_accounts = self.mutate_accounts(static_keys, new_sender)?;
                
                // Convert to a compiled instruction
                let ix = sandwich_sell.to_compiled_instruction(program_id_index)?;
                
                Ok(ix)
            },
            Self::Sell { .. } => Err(MevError::ValueError), // We only create sandwich sells from buy instructions in this example
        }
    }
    
    #[allow(unused)]
    pub fn mint_in(&self, static_keys: &[Pubkey]) -> MevResult<Pubkey> {
        match self {
            Self::Sell { accounts, .. } => Ok(static_keys[accounts[2].account_index as usize]),
            Self::Buy { .. } => Ok(Pubkey::from_str_const("So11111111111111111111111111111111111111112"))
        }
    }
    
    #[allow(unused)]
    pub fn mint_out(&self, static_keys: &[Pubkey]) -> MevResult<Pubkey> {
        match self {
            Self::Buy { accounts, .. } => Ok(static_keys[accounts[2].account_index as usize]),
            Self::Sell { .. } => Ok(Pubkey::from_str_const("So11111111111111111111111111111111111111112"))
        }
    }
    
    pub fn mutate_accounts(&self, static_keys: &[Pubkey], new_sender: &Pubkey) -> MevResult<Vec<Pubkey>> {
        match self {
            Self::Buy { accounts, .. } | Self::Sell { accounts, .. } => {
                Ok(static_keys
                    .iter()
                    .map(|k| {
                        if k == &static_keys[0] { // swap signer
                            return *new_sender
                        } else if k == &static_keys[accounts[5].account_index as usize] { // swap token account
                            return spl_associated_token_account::get_associated_token_address(
                                new_sender, 
                                &static_keys[accounts[2].account_index as usize]
                            )
                        } else if k == &static_keys[accounts[6].account_index as usize] { // swap user account
                            return *new_sender
                        }
                        else {
                            return *k
                        }
                    })
                    .collect())
            }
        }
    }
}

#[cfg(test)]
mod test {
    use solana_sdk::pubkey::Pubkey;

    use crate::programs::Account;
    use super::ParsedPumpFunInstructions;

    #[test]
    fn deserialize_pumpfun_sell_instruction() {
        let sample_ix = [
            51, 230, 133, 164, 1, 127, 131, 173, 
            112, 144, 97, 10, 120, 15, 0, 0, 
            209, 152, 140, 50, 0, 0, 0, 0
        ].to_vec();
        let key_i: Vec<u8> = [16, 2, 10, 3, 4, 1, 0, 11, 13, 12, 17, 15].to_vec();
        let target = ParsedPumpFunInstructions::from_bytes(
            sample_ix, 
            key_i.iter().map(|i| Account::new(i, false)).collect()
        ).unwrap();
        let static_keys: Vec<Pubkey> = [
            "E42sYKJdeWvTaP2yvRWyQFnXhQapqTiAnA8Z9QxBhgjR", 
            "5XCdDpNhuNSDGkNqZZgga1oZz4L7KKLayXHn3ofqgea4", 
            "G5UZAVbAf46s7cKWoyKu8kYTip9DGTpbLZ2qa9Aq69dP", 
            "GNiZwcrcg9Lgf7W6wAgac7UuobeTdcKfLWNgBuEK4Yik", 
            "7K8gLMonWWcBfsCxZ4TnLyn3iDXmDgYBKbN63fnnUJrg", 
            "9RYJ3qr5eU5xAooqVcbmdeusjcViL5Nkiq7Gske3tiKq", 
            "28KqHiudrpzfVkVWQ1jztQ2Aarf4W3CvTitjWEqTCkpA", 
            "ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49", 
            "ComputeBudget111111111111111111111111111111",
            "AFW9KCZtmtMWuhuLkF5mLY9wsk7SZrpZmuKijzcQ51Ni", 
            "EZvAS2D4Y6CcSkiW5wupXK68iyCiVAyfxmDVTsaDpump", 
            "11111111111111111111111111111111", 
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", 
            "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
            "SysvarRent111111111111111111111111111111111", 
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P", 
            "4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf", 
            "Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1", 
            "4pP8eDKACuV7T2rbFPE8CHxGKDYAzSdRsdMsGvz2k4oc", 
            "jitodontfront657582864831789262475769598316"
        ].iter().map(|k| Pubkey::from_str_const(k)).collect();
        assert_eq!(
            target,
            ParsedPumpFunInstructions::Sell {
                discriminator: [51, 230, 133, 164, 1, 127, 131, 173].to_vec(),
                amount: 17008244658288,
                min_sol_output: 848074961,
                accounts: key_i.iter().map(|i| Account::new(i, false)).collect()
            }
        );
        assert_eq!(
            target.mint_in(static_keys.as_slice()).unwrap().to_string().as_str(), 
            "EZvAS2D4Y6CcSkiW5wupXK68iyCiVAyfxmDVTsaDpump"
        );
        assert_eq!(
            target.mint_out(static_keys.as_slice()).unwrap().to_string().as_str(), 
            "So11111111111111111111111111111111111111112"
        );
        let mutated = target.mutate_accounts(
            static_keys.as_slice(), 
            &Pubkey::from_str_const("11111111111111111111111111111111")
        ).unwrap();
        assert_eq!(
            mutated[0],
            Pubkey::from_str_const("11111111111111111111111111111111")
        )
    }
    
    #[test]
    fn deserialize_pumpfun_buy_instruction() {
        let sample_ix = [
            102, 6, 61, 18, 1, 218, 235, 234, 
            27, 162, 85, 43, 0, 0, 0, 0, 
            216, 158, 3, 0, 0, 0, 0, 0
        ].to_vec();
        let key_i: Vec<u8> = [7, 1, 8, 2, 3, 4, 0, 9, 10, 11, 12, 6].to_vec();
        let target = ParsedPumpFunInstructions::from_bytes(
            sample_ix, 
            key_i.iter().map(|i| Account::new(i, false)).collect()
        ).unwrap();
        let static_keys: Vec<Pubkey> = [
            "GCffkajpa2AtfEjC6F9UE8M6GdSroEWjBKyFqqNfLERx", 
            "62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV", 
            "7yq7MVSdiu3uZuMHS8E3c237CLowm1eMT3ieBdC3snXN", 
            "2PqNja9ofayrby8UtV56oUCmQDPQfNnzbjJx3Ut1q85h", 
            "7D5GhqLnWnJAeuQ1Tg2NvdUtRTP22SJb9NLxXY9dwnPo",
            "ComputeBudget111111111111111111111111111111", 
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P", 
            "4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf", 
            "GEG1C8xePLdfnLhua5R53MYcZQVQxtubRzmUGerbpump", 
            "11111111111111111111111111111111",
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", 
            "SysvarRent111111111111111111111111111111111", 
            "Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1"
        ].iter().map(|k| Pubkey::from_str_const(k)).collect();
        assert_eq!(
            target,
            ParsedPumpFunInstructions::Buy {
                discriminator: [102, 6, 61, 18, 1, 218, 235, 234].to_vec(),
                amount: 727032347,
                max_sol_cost: 237272,
                accounts: key_i.iter().map(|i| Account::new(i, false)).collect()
            }
        );
        assert_eq!(
            target.mint_in(static_keys.as_slice()).unwrap().to_string().as_str(), 
            "So11111111111111111111111111111111111111112"
        );
        assert_eq!(
            target.mint_out(static_keys.as_slice()).unwrap().to_string().as_str(), 
            "GEG1C8xePLdfnLhua5R53MYcZQVQxtubRzmUGerbpump"
        );
        let mutated = target.mutate_accounts(
            static_keys.as_slice(), 
            &Pubkey::from_str_const("11111111111111111111111111111111")
        ).unwrap();
        assert_eq!(
            mutated[0],
            Pubkey::from_str_const("11111111111111111111111111111111")
        )
    }
}