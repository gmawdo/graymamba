//use ark_ff::PrimeField;
use ark_bn254::Fr;
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};
use ark_r1cs_std::{
    fields::fp::FpVar,
    alloc::AllocVar,
    eq::EqGadget
};
use super::poseidon_hash::PoseidonHasher;
use std::marker::PhantomData;

pub struct EventCommitmentCircuit {
    // Public inputs (known to witness/verifier)
    pub event_hash: Vec<u8>,
    pub timestamp: i64,
    pub window_id: String,
    pub merkle_root: Vec<u8>,
    pub window_start: i64,
    pub window_end: i64,
    
    // Private inputs (known only to prover)
    pub prover_data: Vec<u8>,
    pub prover_merkle_path: Vec<(Vec<u8>, bool)>,
    
    // System parameters (standardized between prover and verifier)
    pub hasher: PoseidonHasher,
    
    // Phantom data to hold the field type
    _phantom: PhantomData<Fr>
}

impl ConstraintSynthesizer<Fr> for EventCommitmentCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        // 1. Hash verification (already started)
        let event_hash_var = FpVar::<Fr>::new_input(cs.clone(), || {
            Ok(self.hasher.bytes_to_field_element(&self.event_hash))
        })?;
        
        let prover_data_var = FpVar::<Fr>::new_witness(cs.clone(), || {
            Ok(self.hasher.bytes_to_field_element(&self.prover_data))
        })?;

        // Verify hash matches
        let computed_hash = self.hasher.hash_leaf_gadget(cs.clone(), &prover_data_var)?;
        computed_hash.enforce_equal(&event_hash_var)?;

        // 2. Timestamp verification
        let timestamp_var = FpVar::<Fr>::new_input(cs.clone(), || {
            Ok(Fr::from(self.timestamp as u64))
        })?;

        let window_start_var = FpVar::<Fr>::new_input(cs.clone(), || {
            Ok(Fr::from(self.window_start as u64))
        })?;

        let window_end_var = FpVar::<Fr>::new_input(cs.clone(), || {
            Ok(Fr::from(self.window_end as u64))
        })?;

        // Enforce timestamp is within window
        timestamp_var.enforce_cmp(&window_start_var, core::cmp::Ordering::Greater, false)?;
        timestamp_var.enforce_cmp(&window_end_var, core::cmp::Ordering::Less, false)?;

        // TODO: Add Merkle path verification next
        
        Ok(())
    }
}
