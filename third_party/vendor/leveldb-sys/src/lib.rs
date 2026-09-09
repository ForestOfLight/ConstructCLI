use std::{
    ffi::{c_char, c_int},
    os::raw::c_void,
};

#[allow(dead_code)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum Status {
    Success,
    NotFound,
    Corrupted,
    NotSupported,
    InvalidArgument,
    IoError,
    AllocationFailed,
    Exception,
}

#[derive(Debug)]
#[repr(C)]
pub struct Data {
    pub size: c_int,
    pub data: *mut c_void,
}

#[derive(Debug)]
#[repr(C)]
pub struct Result {
    pub status: Status,
    pub size: c_int,
    pub data: *mut c_void,
}

unsafe extern "C" {
    /// Open a LevelDB database.
    ///
    /// # Safety
    ///
    /// This function must receive a valid null-terminated C-style string.
    #[link_name = "bedrockrs_db_open"]
    pub fn db_open(path: *const c_char) -> Result;

    /// Close a LevelDB database.
    /// This also frees the pointers, it must no longer be used.
    ///
    /// # Safety
    ///
    /// `db` must be a pointer previously returned by a call to [`db_open`].
    #[link_name = "bedrockrs_db_close"]
    pub fn db_close(db: *mut c_void);

    /// Loads a value from the database.
    ///
    /// # Safety
    ///
    /// `db` must be a pointer previously returned by a call to [`db_open`],
    /// `key` must be a valid buffer (does not have to be null-terminated) and `key_size`
    /// must be less than or equal to the size of `key`.
    #[link_name = "bedrockrs_db_get"]
    pub fn db_get(db: *mut c_void, key: *const c_char, key_size: c_int) -> Result;

    /// Writes a value into the database.
    ///
    /// # Safety
    ///
    /// `db` must be a pointer previously returned by a call to [`db_open`].
    /// Secondly `key` must be a valid buffer (does not have to be null-terminated) and `key_size`
    /// must be less than or equal to the size of `key`. The `value` and `value_size` must also
    /// satisfy these same conditions.
    #[link_name = "bedrockrs_db_put"]
    pub fn db_put(
        db: *mut c_void,
        key: *const c_char,
        key_size: c_int,
        value: *const c_char,
        value_size: c_int,
    ) -> Result;

    /// Deletes a key from the database.
    ///
    /// # Safety
    ///
    /// `db` must be a pointer previously returned by a call to [`db_open`],
    /// `key` must be a valid buffer (does not have to be null-terminated) and `key_size`
    /// must be less than or equal to the size of `key`.
    #[link_name = "bedrockrs_db_remove"]
    pub fn db_remove(db: *mut c_void, key: *const c_char, key_size: c_int) -> Result;

    /// Deallocates a string previously allocated by another function.
    ///
    /// # Safety
    ///
    /// `array` must be a pointer previously allocated by this FFI code.
    #[link_name = "bedrockrs_buffer_destroy"]
    pub fn buffer_destroy(array: *mut c_char);

    /// Creates an iterator over the database keys.
    ///
    /// # Safety
    ///
    /// `db` must be a pointer previously returned by a call to [`db_open`].
    #[link_name = "bedrockrs_iter_new"]
    pub fn iter_new(db: *mut c_void) -> Result;

    /// Destroys an iterator previously created with [`iter_new`].
    ///
    /// # Safety
    ///
    /// `iter` must be a pointer previously returned by a call to [`iter_new`].
    #[link_name = "bedrockrs_iter_destroy"]
    pub fn iter_destroy(iter: *mut c_void);

    /// Whether the iterator is still valid.
    ///
    /// # Safety
    ///
    /// `iter` must be a pointer previously returned by a call to [`iter_new`].
    #[link_name = "bedrockrs_iter_valid"]
    pub fn iter_valid(iter: *const c_void) -> bool;

    /// The current key the iterator is on.
    ///
    /// # Safety
    ///
    /// `iter` must be a pointer previously returned by a call to [`iter_new`].
    #[link_name = "bedrockrs_iter_key"]
    pub fn iter_key(iter: *const c_void) -> Result;

    /// The current value the iterator is on.
    ///
    /// # Safety
    ///
    /// `iter` must be a pointer previously returned by a call to [`iter_new`].
    #[link_name = "bedrockrs_iter_value"]
    pub fn iter_value(iter: *const c_void) -> Result;

    /// Moves the iterator to the next position.
    ///
    /// # Safety
    ///
    /// `iter` must be a pointer previously returned by a call to [`iter_new`].
    #[link_name = "bedrockrs_iter_next"]
    pub fn iter_next(iter: *mut c_void);

    // /// Creates a new reusable batch.
    // pub fn batch_new() -> *mut c_void;
    // /// Adds a delete operation to the batch.
    // pub fn batch_delete(batch: *mut c_void, key: *const c_char, key_size: c_int);
    // /// Adds a put operation to the batch.
    // pub fn batch_put(batch: *mut c_void, key: *const c_char, key_size: c_int, val: *const c_char, val_size: c_int);
    // /// Clears all operations from the batch.
    // pub fn batch_clear(batch: *mut c_void);
    // /// Deallocates the batch.
    // pub fn batch_destroy(batch: *mut c_void);
    // /// Executes the batch on the provided database
    // pub fn batch_execute(db: *mut c_void, batch: *mut c_void) -> Result;
}
