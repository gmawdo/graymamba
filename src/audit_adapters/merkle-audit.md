# Key Facts

- The current `MerkleBasedAuditSystem` (`merkle_audit.rs`, receives, stores and acknowledges audit events.
- `SharesFS` generates two types of audit events (`sharesbased_fs.rs`):
  - **Disassembly events** (lines 981-992)
  - **Reassembly events** (lines 1069-1079)
- The system already uses RocksDB for storage of the audits.

---

# Our direction of travel

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

# Proposed Solution

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