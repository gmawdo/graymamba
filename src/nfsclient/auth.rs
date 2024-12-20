// Constants for AUTH types
pub const AUTH_UNIX: u32 = 1;
pub const AUTH_NULL: u32 = 0;

// Standard Finder credentials
pub const DEFAULT_UID: u32 = 501;
pub const DEFAULT_GID: u32 = 20;
pub const DEFAULT_AUX_GIDS: [u32; 16] = [
    12, 20, 61, 79, 80, 81, 98, 102, 701, 33, 
    100, 204, 250, 395, 398, 101
];

#[derive(Debug)]
#[allow(dead_code)]
pub struct AuthUnix {
    pub stamp: u32,
    pub machine_name: String,
    pub uid: u32,
    pub gid: u32,
    pub aux_gids: Vec<u32>,
}

impl Default for AuthUnix {
    fn default() -> Self {
        Self {
            stamp: 0,
            machine_name: String::new(),
            uid: DEFAULT_UID,
            gid: DEFAULT_GID,
            aux_gids: DEFAULT_AUX_GIDS.to_vec(),
        }
    }
}

impl AuthUnix {
    pub fn write_to_vec(&self, call: &mut Vec<u8>) {
        // Auth UNIX (flavor = 1)
        call.extend_from_slice(&AUTH_UNIX.to_be_bytes());
        call.extend_from_slice(&84u32.to_be_bytes()); // Length of auth data
        call.extend_from_slice(&self.stamp.to_be_bytes());
        call.extend_from_slice(&0u32.to_be_bytes());  // Machine name length (0)
        call.extend_from_slice(&self.uid.to_be_bytes());
        call.extend_from_slice(&self.gid.to_be_bytes());
        call.extend_from_slice(&(self.aux_gids.len() as u32).to_be_bytes());
        
        // Auxiliary GIDs
        for gid in &self.aux_gids {
            call.extend_from_slice(&gid.to_be_bytes());
        }
        
        // Verifier (AUTH_NULL)
        call.extend_from_slice(&AUTH_NULL.to_be_bytes());
        call.extend_from_slice(&0u32.to_be_bytes());
    }
}