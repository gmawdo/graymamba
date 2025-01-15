use ark_ff::{PrimeField, Field, Zero};
use ark_bn254::Fr;
use ark_serialize::CanonicalSerialize;
use std::error::Error;
use ark_relations::r1cs::{ConstraintSystemRef, SynthesisError};
use ark_r1cs_std::{
    fields::fp::FpVar,
    prelude::*,
    alloc::AllocVar,
};
use std::ops::{Add, Mul};
use tracing::debug;
// Constants for Poseidon
const RATE: usize = 2;
const CAPACITY: usize = 1;
const FULL_ROUNDS: usize = 8;
const PARTIAL_ROUNDS: usize = 57;
const WIDTH: usize = RATE + CAPACITY;

#[derive(Clone)]
pub struct PoseidonHasher {
    state: [Fr; WIDTH],
    round_constants: Vec<[Fr; WIDTH]>,
    mds_matrix: [[Fr; WIDTH]; WIDTH],
}

// Common functions for both regulsr and gadget implementations
impl PoseidonHasher {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let round_constants = Self::generate_round_constants();
        let mds_matrix = Self::generate_mds_matrix();
        
        Ok(Self {
            state: [Fr::zero(); WIDTH],
            round_constants,
            mds_matrix,
        })
    }

    fn field_element_to_bytes(&self, element: &Fr) -> Vec<u8> {
        debug!("Converting field element to bytes:");
        debug!("  element: {:?}", element);
        let mut buf = Vec::new();
        element.serialize_compressed(&mut buf)
            .expect("Serialization should not fail");
        debug!("  resulting bytes: {:?}", buf);
        buf
    }

    fn generate_round_constants() -> Vec<[Fr; WIDTH]> {
        // For testing, generate some simple constants
        // In production, these should be cryptographically secure constants
        let mut constants = Vec::new();
        for i in 0..(FULL_ROUNDS + PARTIAL_ROUNDS) {
            let mut round = [Fr::zero(); WIDTH];
            for (j, round_j) in round.iter_mut().enumerate() {
                // Generate a deterministic but "random-looking" field element
                let seed = (i * WIDTH + j) as u64;
                *round_j = Fr::from(seed);
            }
            constants.push(round);
        }
        constants
    }

    fn generate_mds_matrix() -> [[Fr; WIDTH]; WIDTH] {
        // For testing, generate a simple MDS matrix
        // In production, this should be a proper MDS matrix
        let mut matrix = [[Fr::zero(); WIDTH]; WIDTH];
        for (i, row) in matrix.iter_mut().enumerate() {
            for (j, element) in row.iter_mut().enumerate() {
                // Generate a deterministic but "random-looking" field element
                let seed = (i * WIDTH + j) as u64;
                *element = Fr::from(seed + 1);
            }
        }
        matrix
    }

    pub fn bytes_to_field_element(&self, bytes: &[u8]) -> Fr {
        debug!("Converting bytes to field element:");
        debug!("  bytes: {:?}", bytes);
        let element = Fr::from_be_bytes_mod_order(bytes);
        debug!("  resulting element: {:?}", element);
        element
    }
}

// Regular hashing implementation
impl PoseidonHasher {
    pub fn hash_leaf(&self, data: &[u8]) -> Vec<u8> {
        let element = self.bytes_to_field_element(data);
        let mut state = self.state;
        state[0] = element;
        
        self.permute(&mut state);
        self.field_element_to_bytes(&state[0])
    }

    pub fn hash_nodes(&self, left: &[u8], right: &[u8]) -> Vec<u8> {
        let left_element = self.bytes_to_field_element(left);
        let right_element = self.bytes_to_field_element(right);
        
        let mut state = self.state;
        state[0] = left_element;
        state[1] = right_element;
        
        self.permute(&mut state);
        self.field_element_to_bytes(&state[0])
    }

    fn permute(&self, state: &mut [Fr; WIDTH]) {
        debug!("REGULAR Poseidon permute steps:");
        debug!("  Initial state from round constants: {:?}", state);
        
        // Add round constants
        for i in 0..WIDTH {
            state[i] += self.round_constants[0][i];
        }
        debug!("  After constants: {:?}", state);
        
        // Apply S-box
        for state_i in state.iter_mut() {
            *state_i = state_i.pow(&[5u64]);
        }
        debug!("  After S-box: {:?}", state);
        
        // Mix via MDS matrix
        let old_state = state.to_vec();
        for i in 0..WIDTH {
            state[i] = Fr::zero();
            for j in 0..WIDTH {
                state[i] += old_state[j] * self.mds_matrix[i][j];
            }
        }
        debug!("  After mixing (MDS): {:?}", state);
    }
}

// R1CS gadget implementation
impl PoseidonHasher {
    pub fn hash_leaf_gadget(
        &self,
        cs: ConstraintSystemRef<Fr>,
        leaf: &FpVar<Fr>
    ) -> Result<FpVar<Fr>, SynthesisError> {
        // Get value directly
        let leaf_element = leaf.value().unwrap();
        
        // Create state exactly like regular version
        let mut state_vars = self.state.iter()
            .map(|f| FpVar::<Fr>::new_constant(cs.clone(), *f).unwrap())
            .collect::<Vec<_>>();
        state_vars[0] = FpVar::<Fr>::new_witness(cs.clone(), || Ok(leaf_element))?;
        
        // Use permute_gadget that mirrors regular permute
        self.permute_gadget(&mut state_vars, &cs)?;
        
        // Return first element
        Ok(state_vars[0].clone())
    }

    pub fn hash_nodes_gadget(
        &self,
        cs: ConstraintSystemRef<Fr>,
        left: &FpVar<Fr>,
        right: &FpVar<Fr>
    ) -> Result<FpVar<Fr>, SynthesisError> {
        println!("Setup mode? {}",cs.is_in_setup_mode());
        if !cs.is_in_setup_mode() {
            println!("\nHash nodes gadget:");
            println!("  Left input: {:?}", left.value().unwrap());
            println!("  Right input: {:?}", right.value().unwrap());
        }
        
        let mut state_vars = self.state.iter()
            .map(|f| FpVar::<Fr>::new_constant(cs.clone(), *f).unwrap())
            .collect::<Vec<_>>();
        state_vars[0] = FpVar::<Fr>::new_witness(cs.clone(), || Ok(left.value().unwrap()))?;
        state_vars[1] = FpVar::<Fr>::new_witness(cs.clone(), || Ok(right.value().unwrap()))?;
        
        self.permute_gadget(&mut state_vars, &cs)?;
        
        if cs.is_in_setup_mode() {
            // During setup, return raw hash result
            Ok(state_vars[0].clone())
        } else {
            // Get the result value
            let result_val = state_vars[0].value().unwrap();
            println!("  Result value: {:?}", result_val);
            
            // Convert to bytes and back like regular version
            let result_bytes = self.field_element_to_bytes(&result_val);
            let result_element = self.bytes_to_field_element(&result_bytes);
            println!("  After bytes conversion: {:?}", result_element);

            // Create new witness with the converted result
            let result = FpVar::<Fr>::new_witness(cs.clone(), || Ok(result_element))?;
            
            Ok(result)
        }
    }

    // R1CS permute_gadget
    fn permute_gadget(
        &self,
        state: &mut [FpVar<Fr>],
        cs: &ConstraintSystemRef<Fr>
    ) -> Result<(), SynthesisError> {
        // Add round constants from round 0 only
        for i in 0..WIDTH {
            let round_constant = FpVar::<Fr>::new_constant(cs.clone(), self.round_constants[0][i])?;
            state[i] = state[i].clone().add(&round_constant);
        }
        
        // Apply S-box to all elements using manual x^5 computation
        for state_i in state.iter_mut() {
            let x = state_i.clone();
            let x2 = x.clone().mul(&x);
            let x3 = x2.clone().mul(&x);
            let x4 = x3.clone().mul(&x);
            let x5 = x4.mul(&x);
            *state_i = x5;
        }
        
        // Mix via MDS matrix
        let old_state = state.to_vec();
        for i in 0..WIDTH {
            let mut sum = FpVar::<Fr>::zero();
            for j in 0..WIDTH {
                let mds_element = FpVar::<Fr>::new_constant(cs.clone(), self.mds_matrix[i][j])?;
                let product = old_state[j].clone().mul(&mds_element);
                sum = sum.add(&product);
            }
            state[i] = sum;
        }
        
        Ok(())
    }

} 