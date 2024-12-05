# To prove audit correctness to external witnesses, we now extend our system with ZK-SNARKs.

Based on the high-level requirements in merkle-audit.md (lines 20-22), we need to implement proof of audit trail using zk-snarks
(Plonky2 was an ambition but we decided it is not be stable and tested enough and so we'll use arkworks zk-snarks).

## To recap, here's what we need to prove:

- Existence Proof (already implemented):
  - Event exists in Merkle tree
  - Hash matches stored value

- Audit Trail Integrity (needs to be added into code yet).
  - We need to create a circuit that proves:
  - Events are in chronological order
  - No events have been deleted/modified
  - All required events are present

This implementation would allow external witnesses to:
  - Verify the current Merkle root
  - Check the proof of existence
  - Validate the complete audit trail without seeing the actual data

The ZK proof ensures that there is support for three Types of Verification:
- Quick proof of existence (Merkle proofs)
- Proof of audit trail (ZNP proofs)
- Complete audit trail retrieval (full event data)

## External witnesses
We should always have in mind what an external witness is checking the proof of existence for, and that is:
- The event exists in the Merkle tree at a specific time window (shown in merkle_tree.rs):
- The hash of the event matches the hash in the Merkle tree

The external witness would:
- Receive the Merkle root for a specific time window
- Get a proof that a specific audit event exists in that tree
- Verify the proof without seeing the actual event data

However, this only proves existence at a point in time. To prove the full audit trail is correct we need to add ZK proofs that demonstrate:
- Events are in correct chronological order
- No events have been deleted/modified
- All required events for a given audit trail are present

This requires extending our current proof system to include these additional properties while still maintaining privacy of the actual event data.

### Witness inputs

A witness would know:
- The time window they're interested in (e.g., "24-hour window from Oct 1st 2023")
- The Merkle root for that window
- And from the merkle tree:
  - The hash of the event they want to verify
  - The claimed timestamp of the event

The ZK proof would then prove:
- The event_data hashes to event_hash
- The event exists in the tree with merkle_root
- The timestamp falls within window_start and window_end
- The historical roots form a valid chain

## Prover

The prover uses a commitment to offer up a hash of an event that is recorded

The prover would:
- From merkle_tree code (lines 12-19), we see each event has:
  - A hash
  - A timestamp
  - Event data

Here's the commitment struct:
```rust
struct EventCommitment {
    // Public (given to verifier)
    event_hash: Vec<u8>,      // From MerkleNode.hash
    timestamp: i64,           // From MerkleNode.timestamp
    window_id: String,        // From TimeWindowedMerkleTree window info
    
    // Private (kept by prover)
    event_data: Vec<u8>       // From MerkleNode.event_data
}
```

The idea is to create a circit to prove the commitment(s).

The circuit would need to prove:
- The hash commitment matches the event data
- The timestamp is valid
- The event exists in the tree structure

### Security concerns

Why can't the prover (who writes the system) fix the logic so that any hash given as a commitment to the witness always yield the root hash?
So a potential vulnerability in the proof system.

#### First concern:
The current verification only checks if a hash exists in the tree.

A malicious prover could:
- Take any valid root hash from the tree
- Claim any event hash they want
- Create a fake Merkle path connecting their chosen hash to the root

To prevent this, we would need to:
- In merkle_tree code, we store events with specific structure
- The proof needs to demonstrate:
  - The hash is derived from actual event data
  - The event data follows the required AuditEvent structure
- The Merkle path is valid AND matches the tree structure at that timestamp

This way, a malicious prover can't just make up hashes - they need to provide valid event data that hashes correctly and exists in the correct position in the tree.

#### A deeper concern:

What if the prover writes the code - how does the witness know the code is honest?
The witness needs to trust not just the proofs, but the entire proof generation system.

The system uses Poseidon hash and field arithmetic, but the witness has no guarantee that:
- The prover is using the correct hash function
- The circuit constraints are properly implemented
- The event data is being correctly committed

The solution is that the witness needs:
- A standardized, public circuit specification that defines:
  - The exact Poseidon parameters
  - The field arithmetic operations
  - The commitment scheme
  - The verification constraints

- An independent verifier implementation that:
  - Validates proofs against the public spec
  - Doesn't trust the prover's code
  - Can verify circuit constraints are correct

For example, instead of trusting the prover's implementation of the circuit, the witness should use a standardized verification library that:
- Has been publicly audited
- Implements the agreed-upon circuit spec
- Cannot be modified by the prover

This is where blockchain ecosystems can help. Blockchain system nodes don't trust each other's code, they all implement the same protocol specification independently! The fundamental issue is that even with a circuit, the witness needs to trust that the prover is using the correct circuit implementation.

##### We can solve this by:

- Creating a standardized circuit specification that both parties agree on. The circuit specification should be public and include:
  - The exact Poseidon hash parameters
  - The field arithmetic operations
  - The commitment scheme structure
  - All verification constraints

- The witness would then:
  - Use their own implementation of this specification
  - Verify that proofs follow these exact constraints
  - Not trust any code from the prover

This way, even if we as the prover modify our code, we can't generate valid proofs unless it follows the standardized circuit specification.