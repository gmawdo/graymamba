# Key Facts

- The current `MerkleBasedAuditSystem` (`merkle_audit.rs`, receives, stores and acknowledges audit events.
- `SharesFS` generates two types of audit events (`sharesbased_fs.rs`):
  - **Disassembly events** (lines 981-992)
  - **Reassembly events** (lines 1069-1079)
- The system already uses RocksDB for storage of the audits.

---

# Our direction of travel as of October 2024

1. We need to store both:
   - The Merkle tree structure for verification
   - The actual event data for complete audit trails
2. The system will grow large over time, so we have to consider:
   - **Time-windowed trees**
   - **Archival of older data**
   - **Efficient retrieval methods**
3. Proof of existence and proof of audit trail are both required:
   - **Quick proof of existence** (Merkle proofs)
   - **ZNP proofs** (ZK proofs) for the audit trail using Plonky2
4. Full reporting is required:
   - **Complete audit trail retrieval** (full event data)
5. We need to maintain compatibility with the `IrrefutableAudit` trait.

---

# Proposed Solution as of october 2024

### 1. Use RocksDB Column Families to Separate:
- **Current active tree**  
- **Historical tree roots**  
- **Full event data**  
- **Time-based indices**  

### 2. Implement Time Windows to Manage Data Growth:
- **24-hour active windows**
- **Archival of completed windows**
- Maintain proofs across windows

### 3. Support three Types of Verification:
- **Quick proof of existence** (Merkle proofs)
- **Proof of audit trail** (ZNP proofs)
- **Complete audit trail retrieval** (full event data)

Here's an overview of where we are as off end of Jan 2025 and most of the ambition is now embodied in the `merkle_tree.rs` file with the exception of ZKPs

# What does `merkle_tree.rs` embody?

The `merkle_tree.rs` file is a crucial component of the audit system, implementing a Merkle tree structure to manage and verify audit records over time. The file utilizes four column families in RocksDB to organize data efficiently and maintain the integrity of audit events. Below are the key features and functionalities provided by this file:

## Key Features

### 1. **Merkle Tree Structure**
- Implements a Merkle tree to efficiently store and verify audit records.
- Each node in the tree represents a hash of its child nodes, allowing for quick verification of data integrity.

### 2. **Column Families**
- Utilizes four distinct column families in RocksDB:
  - **current_tree**: Stores the current state of the Merkle tree, representing the latest audit events.
  - **historical_roots**: Maintains a record of the root hashes of previous Merkle trees, allowing for historical verification of audit data.
  - **event_data**: Contains the actual audit event data, which can be referenced by the Merkle nodes.
  - **time_indices**: Keeps track of time windows for which audit events are aggregated into the Merkle tree.

### 3. **Time Window Management**
- Supports the creation of time windows, allowing audit events to be grouped and hashed into a Merkle tree for a specific time period.
- Facilitates the rotation of time windows, where the current tree is finalized and a new tree is initiated for subsequent events.

### 4. **Event Insertion**
- Provides functionality to insert new audit events into the current Merkle tree.
- Each event is hashed and added as a leaf node in the tree, ensuring that the tree structure remains balanced and efficient.

### 5. **Verification of Audit Records**
- Implements methods to verify the integrity of audit records using Merkle proofs.
- Allows for the verification of historical consistency by comparing root hashes across different time windows.

### 6. **Error Handling**
- Incorporates robust error handling to manage issues related to database operations, data integrity, and event processing.

### 7. **Serialization and Deserialization**
- Utilizes serialization (e.g., using `bincode`) to store and retrieve Merkle nodes and audit events from the database efficiently.

## Conclusion
The `merkle_tree.rs` file serves as a foundational component of the audit system, providing a structured approach to managing audit records through the use of a Merkle tree. Its features enable efficient data storage, integrity verification, and historical tracking of audit events, making it an essential part of the overall architecture.

# What is the next step?

The next step is to implement the a R1CS constraint system for the ZKPs. This can work as a single proof for a time window or a series of proofs, one for each event in the time window.
To do a single one for a batch aligns well with other use cases, e.g. proof for a transform in a clinical trial AI pipelin and batch of telemetry from commercial aircraft engines.

Whilst we have a merkle proof for the tree,  R1CS allows us to generate a proof that a the historical audit compuation was performed correctly, and without revealing the underlying data. This ensures that the audit records are constructed from the appropriate data and have not been tampered with.

When using R1CS for batch proofs, a witness can test the audit by providing random audit events, and the verification process is made efficient due to the properties of R1CS. The R1CS system represents the compute as a series of simple polynominals which are agregated into a super polynomial. It can be proven that the proof is correct by evaluating the super polynomial at a set of points. The number of samples needed for verification is related to the order of the super polynomial, allowing for a manageable and efficient verification process. This approach enhances both the security and efficiency of the auditing system, making it feasible to handle large batches of audit events while maintaining integrity and privacy

We will use zkOS from Aleph Zero to define the contraints and create R1CS for each time window batch, implement the circuit, generate the proof, and finally verify it. The proof will be stored on the blockchain alongside the merkle root and the latest historical root.

## Step-by-Step Guide

### 1. Make sure we have the SDK installed

- **Install Aleph Zero SDK**: Follow the instructions on the [Aleph Zero documentation](https://docs.alephzero.org/) to set up the Aleph Zero SDK and zkOS. This may involve adding specific dependencies to the `Cargo.toml` file.

### 2. Define Your Constraints

- **Identify the Logic**: Determine the logic and constraints that need to be represented in our R1CS. This will involve the operations we perform on the audit data, hashing, aggregating events, and verifying integrity.
- **Create a Circuit**: Using zkOS, define a circuit that represents the computation we want to prove. This circuit will include the constraints that must be satisfied for the proof to be valid.

### 3. Implement the Circuit in zkOS

- **Create a New Circuit**: Use the zkOS framework to create the new circuit. This involves defining the inputs, outputs, and the constraints that represent the audit logic.
  
  Example:
  ```rust
  use zk_os::prelude::*;

  struct AuditCircuit {
      // Define inputs and outputs here
      event_data: Vec<u8>,
      timestamp: i64,
      // Other necessary fields
  }

  impl Circuit for AuditCircuit {
      fn synthesize(&self, cs: &mut ConstraintSystem) -> Result<(), SynthesisError> {
          // Define your constraints here
          // Example: cs.enforce(|| "example constraint", |lc| lc + self.timestamp, |lc| lc + 1, |lc| lc + 0);
          Ok(())
      }
  }
  ```

### 4. Batch Processing of Audit Events

- **Collect Events**: Gather the audit events for the time window to process. We will likely do this on a roll over.
- **Create a Batch**: Prepare the batch of events that will be used in the proof generation.

### 5. Generate the R1CS

- **Compile the Circuit**: Use the zkOS tools to compile the circuit into an R1CS representation. This typically involves running a command that processes the Rust code and generates the necessary R1CS files.
  
  Example command (TBD):
  ```bash
  cargo build --release
  ```

### 6. Generate the Proof

- **Create a Prover**: Use the zkOS API to create a prover instance that will generate the proof for a batch of audit events.
  
  Example:
  ```rust
  let prover = Prover::new();
  let proof = prover.prove(&audit_circuit, &inputs)?;
  ```

### 7. Verification

- **Create a Verifier**: Use the zkOS API to create a verifier instance that is able to verify the proof generated for the batch.
  
  Example:
  ```rust
  let verifier = Verifier::new();
  let is_valid = verifier.verify(&proof, &public_inputs)?;
  ```

### 8. Integrate with the Audit Reader Application

- **Store the Proof**: Once verified, store the proof and the associated audit merkle tree root in the database (i.e., RocksDB).
- **Use in the Audit Reader**: Integrate the proof generation and verification into our auditing workflow, ensuring that each time window batch is processed and verified correctly.

This is where we will allow witness data to be submited to the audit reader, and the proof to be verified. The order of the underlying polynomial will be available to the verifier allowing the proof to be exercised for a sufficient number of points to be able to verify the proof is absolutely correct and the audit is complete.