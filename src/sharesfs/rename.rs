use std::sync::{Arc, RwLock};
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use crate::kernel::api::nfs::fileid3;
use crate::kernel::api::nfs::*;
use crate::kernel::api::nfs::nfsstat3;
use super::SharesFS;
use tracing::debug;

use lazy_static::lazy_static;
lazy_static! {
    pub static ref NAMESPACE_ID: Arc<RwLock<String>> = Arc::new(RwLock::new(String::new()));
    pub static ref COMMUNITY: Arc<RwLock<String>> = Arc::new(RwLock::new(String::new()));
}

impl SharesFS {

    pub async fn rename_helper(&self, from_dirid: fileid3, from_filename: &filename3, to_dirid: fileid3, to_filename: &filename3) -> Result<(), nfsstat3> {
        debug!("rename {:?} {:?} {:?} {:?}", from_dirid, from_filename, to_dirid, to_filename);
        let (namespace_id, community) = SharesFS::get_namespace_id_and_community().await;
        
        let from_path: String = self.data_store.hget(
            &format!("{}/{}_id_to_path", community, namespace_id),
            &from_dirid.to_string()
        ).await
            .map_err(|_| nfsstat3::NFS3ERR_IO)?;
        
        let objectname_osstr = OsStr::from_bytes(from_filename).to_os_string();
        // Construct the full path of the file/directory    
        let new_from_path: String = if from_path == "/" {
            format!("/{}", objectname_osstr.to_str().unwrap_or(""))
        } else {
            format!("{}/{}", from_path, objectname_osstr.to_str().unwrap_or(""))
        };

        debug!("rename: {:?} {:?}", from_path, new_from_path);

        // Check if the source file exists in the share store
        let from_exists: bool = match self.data_store.zscore(
            &format!("{}/{}_nodes", community, namespace_id),
            &new_from_path
        ).await {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(e) => {
                eprintln!("Error checking if source file exists: {:?}", e);
                false
            }
        };
        
        if !from_exists {
            return Err(nfsstat3::NFS3ERR_NOENT);
        }

        let to_path: String = self.data_store.hget(
            &format!("{}/{}_id_to_path", community, namespace_id),
            &to_dirid.to_string()
        ).await
            .map_err(|_| nfsstat3::NFS3ERR_IO)?;
        
        let objectname_osstr = OsStr::from_bytes(to_filename).to_os_string();

        // Construct the full path of the file/directory
        let new_to_path: String = if to_path == "/" {
            format!("/{}", objectname_osstr.to_str().unwrap_or(""))
        } else {
            format!("{}/{}", to_path, objectname_osstr.to_str().unwrap_or(""))
        };
            
        let ftype_result = self.get_ftype(new_from_path.clone()).await;
        match ftype_result {
            Ok(ftype) => {
                if ftype == "0" || ftype == "1" || ftype == "2" {
                    debug!("rename_directory_file {:?} {:?}", new_from_path, new_to_path);
                    self.rename_directory_file(&new_from_path, &new_to_path).await?;
                } else {
                    return Err(nfsstat3::NFS3ERR_IO);
                }
            },
            Err(_) => return Err(nfsstat3::NFS3ERR_IO),
            }
            
        Ok(())
    }
}