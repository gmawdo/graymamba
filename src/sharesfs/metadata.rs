use crate::kernel::api::nfs::{fattr3, fileid3, nfsstat3};
use graymamba::file_metadata::FileMetadata;
use crate::sharesfs::SharesFS;
use tracing::debug;

impl SharesFS {
    pub async fn get_attribute(&self, id: fileid3) -> Result<fattr3, nfsstat3> {       
        let metadata = self.get_metadata_from_id(id).await?;
        let path = self.get_path_from_id(id).await?;
        debug!("Stat {:?}: {:?}", path, &metadata);
        FileMetadata::metadata_to_fattr3(id, &metadata).await
    }
}