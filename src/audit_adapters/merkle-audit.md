# Key Facts

- The current `MerkleBasedAuditSystem` (`merkle_audit.rs`, lines 10-53) only receives and acknowledges events without storing them.
- `SharesFS` generates two types of audit events (`sharesbased_fs.rs`):
  - **Disassembly events** (lines 981-992)
  - **Reassembly events** (lines 1069-1079)
- The system already uses RocksDB for storage (`graymamba.rs`, lines 80-81).

---

# Conclusions We Reached

1. We need to store both:
   - The Merkle tree structure for verification
   - The actual event data for complete audit trails
2. The system will grow large over time, so we have considered:
   - **Time-windowed trees**
   - **Archival of older data**
   - **Efficient retrieval methods**
3. We need to maintain compatibility with the `IrrefutableAudit` trait (`irrefutable_audit.rs`, lines 56-83).

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

### 3. Support Two Types of Verification:
- **Quick proof of existence** (Merkle proofs)
- **Complete audit trail retrieval** (full event data)