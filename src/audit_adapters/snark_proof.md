

# Circuit Analysis in `snark_proof.rs`

## Main Circuit
### `EventCommitmentCircuit`
This is the primary circuit that proves three things:
```rust
// Proves:
//  - Hash commitment matches event data
//  - Timestamp is valid
//  - Event exists in the Merkle tree
```

## Helper Circuits
1. **`SimpleTimestampCircuit`**
   - Proves timestamp is within a window
2. **`SimpleMerkleCircuit`**
   - Proves Merkle path validity
3. **`SimpleCircuit`**
   - Basic test circuit that just proves a number equals 1

## Implementation Details
The main circuit (`EventCommitmentCircuit`) uses Poseidon hashing and implements the `ConstraintSynthesizer` trait to generate constraints that prove:
- The event hash matches the actual event data
- The timestamp falls within the specified window
- The Merkle path is valid from the event to the root

## To run the tests
`cargo test audit_adapters::snark_proof::tests --features="merkle_audit,compressed_store,rocksdb_store"`

## Technical Stack
The test suite demonstrates these circuits working with:
- Groth16 proving system
- BN254 curve
- Successfully generating and verifying proofs

## Note on Implementation
The system is currently using arkworks (specifically `ark-bn254` and `ark-groth16`) rather than Plonky2 as mentioned in the spec document.
