//! Product GUI logic (works, staging, backup). The window lives in `main`.

mod work;

pub use work::{
    default_works_root, draft_equal_stages, export_backup, import_backup, read_backup_file, slugify,
    write_backup_file, WorkBackup, WorkEntry, WorkStore,
};
