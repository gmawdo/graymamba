use ark_bn254::Fr;
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};
use ark_r1cs_std::{
    fields::fp::FpVar,
    alloc::AllocVar,
    eq::EqGadget,
};
use super::poseidon_hash::PoseidonHasher;
use std::marker::PhantomData;
#[allow(unused_imports)]
use ark_std::rand::{rngs::StdRng, SeedableRng};
#[allow(unused_imports)]
use crate::audit_adapters::irrefutable_audit::AuditEvent;

#[derive(Clone)]
pub struct SimpleTimestampCircuit {
    pub timestamp: i64,
    pub window_start: i64,
    pub window_end: i64,
    _phantom: PhantomData<Fr>
}

impl ConstraintSynthesizer<Fr> for SimpleTimestampCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        let timestamp_var = FpVar::<Fr>::new_input(cs.clone(), || {
            Ok(Fr::from(self.timestamp as u64))
        })?;

        let window_start_var = FpVar::<Fr>::new_input(cs.clone(), || {
            Ok(Fr::from(self.window_start as u64))
        })?;

        let window_end_var = FpVar::<Fr>::new_input(cs.clone(), || {
            Ok(Fr::from(self.window_end as u64))
        })?;

        // Enforce timestamp is within window (inclusive)
        timestamp_var.enforce_cmp(&window_start_var, core::cmp::Ordering::Greater, true)?;
        timestamp_var.enforce_cmp(&window_end_var, core::cmp::Ordering::Less, true)?;

        Ok(())
    }
}

#[derive(Clone)]
#[allow(dead_code)] //used in a test
struct SimpleCircuit {
    // Public input
    pub number: u64,
    _phantom: PhantomData<Fr>,
}

impl ConstraintSynthesizer<Fr> for SimpleCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        // Create variable for our public input
        let a = FpVar::<Fr>::new_input(cs.clone(), || Ok(Fr::from(self.number)))?;
        
        // Create constant 1
        let one = FpVar::<Fr>::new_constant(cs.clone(), Fr::from(1u64))?;
        
        // Assert that a equals 1
        a.enforce_equal(&one)?;
        
        Ok(())
    }
}

#[derive(Clone)]
pub struct SimpleMerkleCircuit {
    // Public inputs
    pub event_hash: Vec<u8>,
    pub merkle_root: Vec<u8>,
    
    // Private inputs
    pub merkle_path: Vec<(Vec<u8>, bool)>,
    
    // System parameters
    pub hasher: PoseidonHasher,
    _phantom: PhantomData<Fr>
}

impl ConstraintSynthesizer<Fr> for SimpleMerkleCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        let event_hash_var = FpVar::<Fr>::new_input(cs.clone(), || {
            Ok(self.hasher.bytes_to_field_element(&self.event_hash))
        })?;

        let merkle_root_var = FpVar::<Fr>::new_input(cs.clone(), || {
            Ok(self.hasher.bytes_to_field_element(&self.merkle_root))
        })?;

        // Verify the Merkle path
        let computed_root = self.verify_merkle_path(cs.clone(), &event_hash_var, &self.merkle_path)?;
        computed_root.enforce_equal(&merkle_root_var)?;

        Ok(())
    }
}

impl SimpleMerkleCircuit {
    fn verify_merkle_path(
        &self,
        cs: ConstraintSystemRef<Fr>,
        leaf_hash: &FpVar<Fr>,
        merkle_path: &Vec<(Vec<u8>, bool)>
    ) -> Result<FpVar<Fr>, SynthesisError> {
        let mut current_hash = leaf_hash.clone();

        for (sibling_hash, is_left) in merkle_path {
            // Create sibling variable
            let sibling_element = self.hasher.bytes_to_field_element(sibling_hash);
            let sibling_var = FpVar::<Fr>::new_witness(cs.clone(), || Ok(sibling_element))?;
            
            // Compute new hash
            current_hash = if *is_left {
                self.hasher.hash_nodes_gadget(cs.clone(), &sibling_var, &current_hash)?
            } else {
                self.hasher.hash_nodes_gadget(cs.clone(), &current_hash, &sibling_var)?
            };
        }

        Ok(current_hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_bn254::Bn254;
    use ark_groth16::Groth16;
    use ark_snark::SNARK;


    #[tokio::test]
    async fn test_simple_circuit() -> Result<(), Box<dyn std::error::Error>> {
        let mut rng = StdRng::seed_from_u64(0u64);
        
        println!("\n1. Creating simple circuit...");
        let circuit = SimpleCircuit {
            number: 1u64,
            _phantom: PhantomData,
        };
        
        println!("2. Setting up circuit proof system...");
        let (pk, vk) = Groth16::<Bn254>::circuit_specific_setup(circuit.clone(), &mut rng)?;
        
        println!("3. Generating proof...");
        let proof = Groth16::<Bn254>::prove(&pk, circuit, &mut rng)?;
        
        println!("4. Verifying proof...");
        let public_inputs = vec![Fr::from(1u64)];
        let is_valid = Groth16::<Bn254>::verify(&vk, &public_inputs, &proof)?;
        
        assert!(is_valid, "Proof verification failed!");
        println!("5. Proof verified successfully!\n\n");
        
        Ok(())
    }
    #[tokio::test]
    async fn test_simple_timestamp_circuit() -> Result<(), Box<dyn std::error::Error>> {
        let mut rng = StdRng::seed_from_u64(0u64);

        // Use the same timestamp values from the main test
        let timestamp = 1696161600i64;      // Oct 1, 2023 12:00:00
        let window_start = 1696118400i64;   // Oct 1, 2023 00:00:00
        let window_end = 1696204799i64;     // Oct 1, 2023 23:59:59

        println!("1. Creating simple timestamp circuit...");
        let circuit = SimpleTimestampCircuit {
            timestamp,
            window_start,
            window_end,
            _phantom: PhantomData,
        };

        println!("2. Setting up constraint system...");
        let cs = ark_relations::r1cs::ConstraintSystem::<Fr>::new_ref();
        cs.set_optimization_goal(ark_relations::r1cs::OptimizationGoal::Constraints);
        cs.set_mode(ark_relations::r1cs::SynthesisMode::Prove { construct_matrices: true });

        println!("3. Generating constraints...");
        circuit.clone().generate_constraints(cs.clone())?;

        let satisfied = cs.is_satisfied()?;
        println!("4. Constraint satisfaction: {}", satisfied);
        println!("5. Number of constraints: {}", cs.num_constraints());

        // Only proceed if constraints are satisfied
        //they are satified when the witness values are set and the circuit is run successfully
        assert!(satisfied, "Constraints not satisfied!");

        // now we have to generate the proof and verify it - that's the prover's job
        let (pk, vk) = Groth16::<Bn254>::circuit_specific_setup(circuit.clone(), &mut rng)?;
        println!("6. Generated proving and verifying keys");

        let proof = Groth16::<Bn254>::prove(&pk, circuit, &mut rng)?;
        println!("7. Generated proof");

        let public_inputs = vec![
            Fr::from(timestamp as u64),
            Fr::from(window_start as u64),
            Fr::from(window_end as u64),
        ];

        let is_valid = Groth16::<Bn254>::verify(&vk, &public_inputs, &proof)?;
        println!("8. Verification result: {}", is_valid);

        Ok(())
    }
    #[tokio::test]
    async fn test_simple_merkle_circuit() -> Result<(), Box<dyn std::error::Error>> {
        let mut rng = StdRng::seed_from_u64(0u64);
        let hasher = PoseidonHasher::new()?;

        // Create test data
        let event_hash = hasher.hash_leaf(b"test_event");
        let sibling1 = hasher.hash_leaf(b"sibling1");
        let sibling2 = hasher.hash_leaf(b"sibling2");
        
        println!("1. Created hashes:");
        println!("   event_hash: {:?}", event_hash);
        println!("   sibling1: {:?}", sibling1);
        println!("   sibling2: {:?}", sibling2);
        
        // Calculate root
        let intermediate = hasher.hash_nodes(&event_hash, &sibling1);
        let merkle_root = hasher.hash_nodes(&intermediate, &sibling2);
        
        println!("2. Calculated intermediate and root:");
        println!("   intermediate: {:?}", intermediate);
        println!("   root: {:?}", merkle_root);

        let merkle_path = vec![
            (sibling1.clone(), false),
            (sibling2.clone(), false),
        ];

        // Create circuit
        let circuit = SimpleMerkleCircuit {
            event_hash: event_hash.clone(),
            merkle_root: merkle_root.clone(),
            merkle_path,
            hasher: hasher.clone(),
            _phantom: PhantomData,
        };

        // Set up constraint system
        println!("3. Setting up constraint system...");
        let cs = ark_relations::r1cs::ConstraintSystem::<Fr>::new_ref();
        cs.set_optimization_goal(ark_relations::r1cs::OptimizationGoal::Constraints);
        cs.set_mode(ark_relations::r1cs::SynthesisMode::Prove { construct_matrices: true });

        // Generate and check constraints
        let cs_clone = cs.clone();
        circuit.clone().generate_constraints(cs.clone())?;

        let satisfied = cs.is_satisfied()?;
        println!("4. Constraint satisfaction: {}", satisfied);
        println!("5. Constraint System Info:");
        println!("   Num constraints: {}", cs_clone.num_constraints());
        println!("   Num instance variables: {}", cs_clone.num_instance_variables());
        println!("   Num witness variables: {}", cs_clone.num_witness_variables());

        assert!(satisfied, "Constraints not satisfied!");

        // Generate and verify proof
        let (pk, vk) = Groth16::<Bn254>::circuit_specific_setup(circuit.clone(), &mut rng)?;
        println!("6. Generated proving and verifying keys");

        let proof = Groth16::<Bn254>::prove(&pk, circuit, &mut rng)?;
        println!("7. Generated proof");

        let public_inputs = vec![
            hasher.bytes_to_field_element(&event_hash),
            hasher.bytes_to_field_element(&merkle_root),
        ];

        let is_valid = Groth16::<Bn254>::verify(&vk, &public_inputs, &proof)?;
        println!("8. Verification result: {}", is_valid);

        Ok(())
    }
}
