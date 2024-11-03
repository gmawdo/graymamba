use graymamba::nfs::fileid3;
use std::os::unix::fs::PermissionsExt;
use graymamba::nfs::nfsstat3;
use graymamba::nfs::ftype3;
use graymamba::nfs::specdata3;
use graymamba::nfs::nfstime3;
use graymamba::nfs::fattr3;

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
    #[allow(dead_code)]
    pub fn new(ftype: u8, permissions: u32, size: u64, access_time_secs: u32, access_time_nsecs: u32, change_time_secs: u32, change_time_nsecs: u32, modification_time_secs: u32, modification_time_nsecs: u32, fileid: u64) -> Self {
        FileMetadata {
            ftype,
            permissions,
            size,
            access_time_secs,
            access_time_nsecs,
            change_time_secs,
            change_time_nsecs,
            modification_time_secs,
            modification_time_nsecs,
            fileid,
        }
    }
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
                seconds: metadata.access_time_secs as u32,
                nseconds: metadata.access_time_nsecs as u32,
            },
            mtime: nfstime3 {
                seconds: metadata.modification_time_secs as u32,
                nseconds: metadata.modification_time_nsecs as u32,
            },
            ctime: nfstime3 {
                seconds: metadata.change_time_secs as u32,
                nseconds: metadata.change_time_nsecs as u32,
            },
        })
    }
}