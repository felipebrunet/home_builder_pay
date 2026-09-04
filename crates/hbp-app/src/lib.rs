//! Product GUI logic (works, staging, backup). The window lives in `main`.

mod datetime;
mod work;

pub use datetime::{
    format_unix_local_es, validate_deadline_order, DeadlineFields, MONTHS_ES,
};
pub use work::{
    default_works_root, draft_equal_stages, export_backup, import_backup, read_backup_file, slugify,
    write_backup_file, UiPrefs, WorkBackup, WorkEntry, WorkStore,
};
