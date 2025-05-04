use serde::{Serialize, Deserialize};
use std::convert::TryFrom;
use std::io::{Cursor, Read};
use std::collections::HashMap;
use lazy_static::lazy_static;

use crate::result::MevError;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GenericValue<T> {
    pub data: T,
    #[serde(rename(deserialize = "type"))]
    pub _type: String
}

impl<T: Copy> GenericValue<T> {
    pub fn new(data: T, type_name: &str) -> Self {
        Self {
            data,
            _type: type_name.to_string(),
        }
    }
    
    // Helper for serialization
    pub fn to_raw_bytes(&self) -> Result<Vec<u8>, MevError> {
        match self._type.as_str() {
            "u8" => {
                // Directly cast to u8
                let value = unsafe { std::mem::transmute_copy::<T, u8>(&self.data) };
                Ok(vec![value])
            },
            "u32" => {
                // Cast to u32 and convert to bytes
                let value = unsafe { std::mem::transmute_copy::<T, u32>(&self.data) };
                Ok(value.to_le_bytes().to_vec())
            },
            "u64" => {
                // Cast to u64 and convert to bytes
                let value = unsafe { std::mem::transmute_copy::<T, u64>(&self.data) };
                Ok(value.to_le_bytes().to_vec())
            },
            _ => Err(MevError::FailedToSerialize)
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum TxInstructions {
    SetComputeUnitLimit {
        discriminator: GenericValue<u8>,
        units: GenericValue<u32>
    },
    SetComputeUnitPrice {
        discriminator: GenericValue<u8>,
        #[serde(rename(deserialize = "microLamports"))]
        micro_lamports: GenericValue<u64>
    },
    PumpFunSell {
        // No discriminator field as per Solscan representation
        base_amount_in: GenericValue<u64>,
        min_quote_amount_out: GenericValue<u64>
    }
}

// Type alias for the deserialization handler function
type DeserializeFn = fn(&mut Cursor<&[u8]>) -> Result<TxInstructions, MevError>;

// Registry of instruction discriminators and their deserialization handlers
lazy_static! {
    static ref INSTRUCTION_REGISTRY: HashMap<u8, DeserializeFn> = {
        let mut registry: HashMap<u8, DeserializeFn> = HashMap::new();
        
        // Register instruction handlers
        registry.insert(2, deserialize_set_compute_unit_limit as DeserializeFn);
        registry.insert(3, deserialize_set_compute_unit_price as DeserializeFn);
        registry.insert(0x33, deserialize_pump_fun_sell as DeserializeFn); // 0x33 (51) for PumpFunSell
        
        registry
    };
}

// Deserialization handler for SetComputeUnitLimit
fn deserialize_set_compute_unit_limit(cursor: &mut Cursor<&[u8]>) -> Result<TxInstructions, MevError> {
    let discriminator = 2u8;
    
    // Read u32 units value (4 bytes)
    let mut units_bytes = [0u8; 4];
    cursor.read_exact(&mut units_bytes)
        .map_err(|_| MevError::FailedToDeserialize)?;
    let units = u32::from_le_bytes(units_bytes);
    
    Ok(TxInstructions::SetComputeUnitLimit {
        discriminator: GenericValue::new(discriminator, "u8"),
        units: GenericValue::new(units, "u32"),
    })
}

// Deserialization handler for SetComputeUnitPrice
fn deserialize_set_compute_unit_price(cursor: &mut Cursor<&[u8]>) -> Result<TxInstructions, MevError> {
    let discriminator = 3u8;
    
    // Read u64 microLamports value (8 bytes)
    let mut micro_lamports_bytes = [0u8; 8];
    cursor.read_exact(&mut micro_lamports_bytes)
        .map_err(|_| MevError::FailedToDeserialize)?;
    let micro_lamports = u64::from_le_bytes(micro_lamports_bytes);
    
    Ok(TxInstructions::SetComputeUnitPrice {
        discriminator: GenericValue::new(discriminator, "u8"),
        micro_lamports: GenericValue::new(micro_lamports, "u64"),
    })
}

// Deserialization handler for PumpFunSell
fn deserialize_pump_fun_sell(cursor: &mut Cursor<&[u8]>) -> Result<TxInstructions, MevError> {
    // Read base_amount_in (first u64, 8 bytes)
    let mut base_amount_in_bytes = [0u8; 8];
    cursor.read_exact(&mut base_amount_in_bytes)
        .map_err(|_| MevError::FailedToDeserialize)?;
    let base_amount_in = u64::from_le_bytes(base_amount_in_bytes);
    
    // Read min_quote_amount_out (second u64, 8 bytes)
    let mut min_quote_amount_out_bytes = [0u8; 8];
    cursor.read_exact(&mut min_quote_amount_out_bytes)
        .map_err(|_| MevError::FailedToDeserialize)?;
    let min_quote_amount_out = u64::from_le_bytes(min_quote_amount_out_bytes);
    
    // Read any additional data (if present) but we don't store it
    // This helps handle the full binary format even if we don't use all the data
    let mut remaining_data = Vec::new();
    cursor.read_to_end(&mut remaining_data)
        .map_err(|_| MevError::FailedToDeserialize)?;
    
    Ok(TxInstructions::PumpFunSell {
        base_amount_in: GenericValue::new(base_amount_in, "u64"),
        min_quote_amount_out: GenericValue::new(min_quote_amount_out, "u64"),
    })
}

impl TxInstructions {
    /// Deserialize TxInstructions from JSON bytes
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, MevError> {
        let instructions = serde_json::from_slice(bytes)
            .map_err(|_| MevError::FailedToDeserialize)?;
        Ok(instructions)
    }
    
    /// Deserialize TxInstructions from bincode bytes
    pub fn from_bincode_bytes(bytes: &[u8]) -> Result<Self, MevError> {
        let instructions = bincode::deserialize(bytes)
            .map_err(|_| MevError::FailedToDeserialize)?;
        Ok(instructions)
    }
    
    /// Convert a hex string to TxInstructions
    #[allow(unused)]
    pub fn from_hex(hex_str: &str) -> Result<Self, MevError> {
        let bytes = hex::decode(hex_str).map_err(|_| MevError::FailedToDeserialize)?;
        Self::try_from(bytes)
    }
    
    /// Convert TxInstructions to a hex string (for debugging)
    #[allow(unused)]
    pub fn to_hex(&self) -> Result<String, MevError> {
        let bytes = self.to_raw_bytes()?;        
        Ok(hex::encode(bytes))
    }
    
    /// Deserialize from raw binary format where:
    /// - First byte is the discriminator
    /// - Remaining bytes are the payload for that instruction type
    pub fn from_raw_bytes(bytes: &[u8]) -> Result<Self, MevError> {
        if bytes.is_empty() {
            return Err(MevError::FailedToDeserialize);
        }

        let mut cursor = Cursor::new(bytes);
        
        // Read the first byte as discriminator
        let mut discriminator_bytes = [0u8; 1];
        cursor.read_exact(&mut discriminator_bytes)
            .map_err(|_| MevError::FailedToDeserialize)?;
        let discriminator = discriminator_bytes[0];
        
        // Look up the deserialization handler for this discriminator
        match INSTRUCTION_REGISTRY.get(&discriminator) {
            Some(deserialize_fn) => deserialize_fn(&mut cursor),
            None => Err(MevError::FailedToDeserialize),
        }
    }
    
    pub fn to_raw_bytes(&self) -> Result<Vec<u8>, MevError> {
        match self {
            TxInstructions::SetComputeUnitLimit { discriminator, units } => {
                let mut bytes = Vec::new();
                bytes.extend(discriminator.to_raw_bytes()?);
                bytes.extend(units.to_raw_bytes()?);
                Ok(bytes)
            },
            TxInstructions::SetComputeUnitPrice { discriminator, micro_lamports } => {
                let mut bytes = Vec::new();
                bytes.extend(discriminator.to_raw_bytes()?);
                bytes.extend(micro_lamports.to_raw_bytes()?);
                Ok(bytes)
            },
            TxInstructions::PumpFunSell { base_amount_in, min_quote_amount_out } => {
                let mut bytes = Vec::new();
                // Add the discriminator byte for PumpFunSell (0x33)
                bytes.push(0x33);
                bytes.extend(base_amount_in.to_raw_bytes()?);
                bytes.extend(min_quote_amount_out.to_raw_bytes()?);
                // We'd add any additional data at the end if needed
                Ok(bytes)
            },
        }
    }
    
    // Remove the test helper for now as it's not needed and would require more complex handling
    // We'll rely on the registry initialization to handle all instructions
}

impl TryFrom<Vec<u8>> for TxInstructions {
    type Error = MevError;
    
    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        Self::from_raw_bytes(&bytes)
            .or_else(|_| Self::from_json_bytes(&bytes))
            .or_else(|_| Self::from_bincode_bytes(&bytes))
    }
}