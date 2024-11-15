use graymamba::nfs;
use nfs::fileid3;
use std::os::unix::fs::PermissionsExt;
use nfs::nfsstat3;
use nfs::ftype3;
use nfs::specdata3;
use nfs::nfstime3;
use nfs::fattr3;

#[derive(Debug, Clone)]
pub struct FileMetadata {

    // Common metadata fields
    pub ftype: u8,      // 0 for directory, 1 for file, 2 for Sybolic link
    pub size: u64,
    pub permissions: u32,
    pub access_time_secs: u32,
    pub access_time_nsecs: u32,
    pub change_time_secs: u32,
    pub change_time_nsecs: u32,
    pub modification_time_secs: u32,
    pub modification_time_nsecs: u32,
    pub fileid: fileid3,

}

impl FileMetadata {
    async fn mode_unmask(mode: u32) -> u32 {
        let mode = mode | 0x80;
        let permissions = std::fs::Permissions::from_mode(mode);
        permissions.mode() & 0x1FF
    }
    pub async fn metadata_to_fattr3(fid: fileid3, metadata: &FileMetadata) -> Result<fattr3, nfsstat3> {
        let size = metadata.size;
        let file_mode = Self::mode_unmask(metadata.permissions);
        let ftype = match metadata.ftype {
            0 => ftype3::NF3DIR,
            1 => ftype3::NF3REG,
            2 => ftype3::NF3LNK,
            _ => return Err(nfsstat3::NFS3ERR_INVAL),
        };
        
        Ok(fattr3 {
            ftype,
            mode: file_mode.await,
            nlink: 1,
            uid: 0,
            gid: 0,
            size,
            used: size,
            fsid: 0,
            fileid: fid,
            rdev: specdata3::default(),
            atime: nfstime3 {
                seconds: metadata.access_time_secs,
                nseconds: metadata.access_time_nsecs,
            },
            mtime: nfstime3 {
                seconds: metadata.modification_time_secs,
                nseconds: metadata.modification_time_nsecs,
            },
            ctime: nfstime3 {
                seconds: metadata.change_time_secs,
                nseconds: metadata.change_time_nsecs,
            },
        })
    }
}