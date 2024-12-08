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

    pub fn hash_leaf_gadget(
        &self,
        cs: ConstraintSystemRef<Fr>,
        input: &FpVar<Fr>
    ) -> Result<FpVar<Fr>, SynthesisError> {
        // Initialize state variables
        let mut state_vars = vec![FpVar::<Fr>::zero(); WIDTH];
        state_vars[0] = input.clone();

        // Apply the Poseidon permutation
        // First half of full rounds
        for r in 0..FULL_ROUNDS/2 {
            self.full_round_gadget(&mut state_vars, r, &cs)?;
        }
        
        // Partial rounds
        for r in 0..PARTIAL_ROUNDS {
            self.partial_round_gadget(&mut state_vars, FULL_ROUNDS/2 + r, &cs)?;
        }
        
        // Second half of full rounds
        for r in 0..FULL_ROUNDS/2 {
            self.full_round_gadget(&mut state_vars, FULL_ROUNDS/2 + PARTIAL_ROUNDS + r, &cs)?;
        }

        Ok(state_vars[0].clone())
    }

    pub fn hash_nodes_gadget(
        &self,
        cs: ConstraintSystemRef<Fr>,
        left: &FpVar<Fr>,
        right: &FpVar<Fr>
    ) -> Result<FpVar<Fr>, SynthesisError> {
        // Initialize state with input nodes
        let mut state_vars = vec![FpVar::<Fr>::zero(); WIDTH];
        state_vars[0] = left.clone();
        state_vars[1] = right.clone();
        
        // Apply the Poseidon permutation
        // First half of full rounds
        for r in 0..FULL_ROUNDS/2 {
            self.full_round_gadget(&mut state_vars, r, &cs)?;
        }
        
        // Partial rounds
        for r in 0..PARTIAL_ROUNDS {
            self.partial_round_gadget(&mut state_vars, FULL_ROUNDS/2 + r, &cs)?;
        }
        
        // Second half of full rounds
        for r in 0..FULL_ROUNDS/2 {
            self.full_round_gadget(&mut state_vars, FULL_ROUNDS/2 + PARTIAL_ROUNDS + r, &cs)?;
        }

        // Return first element as hash output
        Ok(state_vars[0].clone())
    }

    fn full_round_gadget(
        &self,
        state: &mut [FpVar<Fr>],
        round: usize,
        cs: &ConstraintSystemRef<Fr>
    ) -> Result<(), SynthesisError> {

        // Add round constants
        for (i, state_i) in state.iter_mut().enumerate() {
            let constant = FpVar::<Fr>::new_constant(cs.clone(), self.round_constants[round][i])?;
            let current_state = state_i.clone();
            *state_i = current_state.add(&constant);
        }
        
        // Apply S-box (x^5) to all elements
        for state_i in state.iter_mut() {
            let current_state = state_i.clone();
            *state_i = current_state.pow_by_constant([5u64])?;
        }
        
        // Apply MDS matrix
        let old_state = state.to_vec();
        for (i, state_i) in state.iter_mut().enumerate() {
            *state_i = FpVar::<Fr>::zero();
            for (j, old_state_j) in old_state.iter().enumerate() {
                let mds_element = FpVar::<Fr>::new_constant(cs.clone(), self.mds_matrix[i][j])?;
                let product = old_state_j.clone().mul(&mds_element);
                let current_sum = state_i.clone();
                *state_i = current_sum.add(&product);
            }
        }
        
        Ok(())
    }

    fn partial_round_gadget(
        &self,
        state: &mut [FpVar<Fr>],
        round: usize,
        cs: &ConstraintSystemRef<Fr>
    ) -> Result<(), SynthesisError> {
        // Add round constants
        for (i, state_i) in state.iter_mut().enumerate() {
            let constant = FpVar::<Fr>::new_constant(cs.clone(), self.round_constants[round][i])?;
            let current_state = state_i.clone();
            *state_i = current_state.add(&constant);
        }
        
        // Apply S-box only to first element
        let current_state = state[0].clone();
        state[0] = current_state.pow_by_constant([5u64])?;
        
        // Apply MDS matrix
        let old_state = state.to_vec();
        for (i, state_i) in state.iter_mut().enumerate() {
            *state_i = FpVar::<Fr>::zero();
            for (j, old_state_j) in old_state.iter().enumerate() {
                let mds_element = FpVar::<Fr>::new_constant(cs.clone(), self.mds_matrix[i][j])?;
                let product = old_state_j.clone().mul(&mds_element);
                let current_sum = state_i.clone();
                *state_i = current_sum.add(&product);
            }
        }
        
        Ok(())
    }

    // The following functions in PoseidonHasher are internal implementation details that support the two public functions above:
    // permute()
    // full_round()
    // partial_round()
    // bytes_to_field_element()
    // field_element_to_bytes()
    // generate_round_constants()
    // generate_mds_matrix()

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

    fn permute(&self, state: &mut [Fr; WIDTH]) {
        // First half of full rounds
        for r in 0..FULL_ROUNDS/2 {
            self.full_round(state, r);
        }
        
        // Partial rounds
        for r in 0..PARTIAL_ROUNDS {
            self.partial_round(state, FULL_ROUNDS/2 + r);
        }
        
        // Second half of full rounds
        for r in 0..FULL_ROUNDS/2 {
            self.full_round(state, FULL_ROUNDS/2 + PARTIAL_ROUNDS + r);
        }
    }

    fn full_round(&self, state: &mut [Fr; WIDTH], round: usize) {
        // Add round constants
        for (i, state_i) in state.iter_mut().enumerate() {
            *state_i += self.round_constants[round][i];
        }
        
        // Apply S-box to all elements
        for state_i in state.iter_mut() {
            *state_i = state_i.pow([5u64]);  // x^5 S-box
        }
        
        // Apply MDS matrix
        let old_state = *state;
        for (i, state_i) in state.iter_mut().enumerate() {
            *state_i = Fr::zero();
            for (j, old_state_j) in old_state.iter().enumerate() {
                *state_i += self.mds_matrix[i][j] * old_state_j;
            }
        }
    }

    fn partial_round(&self, state: &mut [Fr; WIDTH], round: usize) {
        // Add round constants
        for (i, state_i) in state.iter_mut().enumerate() {
            *state_i += self.round_constants[round][i];
        }
        
        // Apply S-box only to first element
        state[0] = state[0].pow([5u64]);
        
        // Apply MDS matrix
        let old_state = *state;
        for (i, state_i) in state.iter_mut().enumerate() {
            *state_i = Fr::zero();
            for (j, old_state_j) in old_state.iter().enumerate() {
                *state_i += self.mds_matrix[i][j] * old_state_j;
            }
        }
    }

    pub fn bytes_to_field_element(&self, bytes: &[u8]) -> Fr {
        Fr::from_be_bytes_mod_order(bytes)
    }

    fn field_element_to_bytes(&self, element: &Fr) -> Vec<u8> {
        let mut buf = Vec::new();
        element.serialize_compressed(&mut buf)
            .expect("Serialization should not fail");
        buf
    }
} 