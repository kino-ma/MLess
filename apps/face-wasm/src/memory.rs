use std::slice;

use prost::Message;

#[cfg(target_family = "wasm")]
const PAGE_SIZE: usize = 65536;

pub struct Memory {
    head: *const u8,
    last: *const u8,
}

impl Memory {
    pub fn new<P: Into<*const u8>, L: Into<*const u8>>(ptr: P, last: L) -> Self {
        let head = ptr.into();
        let last = last.into();

        Self { head, last }
    }

    pub fn with_used_len<P: Into<*const u8>, L: Into<i64>>(ptr: P, len: L) -> Self {
        let ptr: *const u8 = ptr.into();
        let len: i64 = len.into();
        let last = unsafe { ptr.add(len as _) };

        Self::new(ptr, last)
    }

    pub fn len(&self) -> usize {
        let offset = unsafe { self.last.offset_from(self.head) };
        offset as _
    }

    pub unsafe fn get_slice<L: Into<usize>, T>(&self, start: *const T, len: L) -> &[T] {
        slice::from_raw_parts(start, len.into())
    }

    pub fn get_whole<T>(&self) -> &[T] {
        unsafe { self.get_slice(self.head as _, self.len()) }
    }

    pub fn write_str(&mut self, data: &str) -> &str {
        let bytes = self.write_bytes(data.as_bytes());
        core::str::from_utf8(bytes).unwrap()
    }

    pub fn write_bytes<'a, 'b>(&'a mut self, data: &'b [u8]) -> &'a [u8] {
        #[cfg(target_family = "wasm")]
        self.grow_to(data.len());

        let start: *mut u8 = unsafe { self.last.add(1).cast_mut() };
        let len = data.len();
        unsafe {
            std::ptr::copy(data.as_ptr(), start, len);
            self.last = self.last.add(len);
            self.get_slice(start, len)
        }
    }

    #[cfg(target_family = "wasm")]
    fn grow_to(&self, data_len: usize) -> usize {
        use core::arch;
        let current_size = arch::wasm32::memory_size(0) as usize;
        let cap = current_size * PAGE_SIZE;

        let len = self.len();
        assert!(len <= cap);

        let start = len + 1;
        let available = cap - start;
        let missing = data_len - available;
        if missing > 0 {
            let to_grow = missing / PAGE_SIZE + 1;
            arch::wasm32::memory_grow(0, to_grow as _);
            to_grow
        } else {
            0
        }
    }
}

pub fn slice_to_i64(s: &[u8]) -> i64 {
    let ptr = (s.as_ptr() as i64) << 32;
    let len = s.len() as i64;

    ptr | len
}

/// For benchmarking purpose
#[cfg(feature = "bench")]
#[cfg_attr(not(test), no_mangle)]
pub extern "C" fn read_detection_request(start: i32, len: i32) -> i64 {
    use crate::{
        face_proto::DetectionRequest,
        host_proto::{invoke_result::Result as IResult, Finished, InvokeResult, MemorySlice},
    };

    let mut mem = Memory::with_used_len(start as *const u8, len);
    let buffer = mem.get_whole();

    let msg: DetectionRequest = Message::decode(buffer)
        .map_err(|e| format!("ERR: Failed to decode request: {e}"))
        .unwrap();

    let data = format!("{}", msg.image_png.len());
    mem.write_str(&data);

    let res = InvokeResult {
        result: Some(IResult::Finished(Finished {
            ptr: Some(MemorySlice {
                start: data.as_ptr() as _,
                len: data.len() as _,
            }),
        })),
    };

    let res_data = res.encode_to_vec();
    let slic = mem.write_bytes(&res_data);
    slice_to_i64(slic)
}

pub fn read_message<M: Message + Default>(buffer: &[u8]) -> Result<M, String> {
    Message::decode(buffer).map_err(|e| format!("ERR: Failed to decode request: {e}"))
}
