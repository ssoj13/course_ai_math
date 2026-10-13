//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/openjph/ojph_mem.h`

use crate::openjph::ojph_arch::{BYTE_ALIGNMENT, OBJECT_ALIGNMENT, align_ptr, calc_aligned_size};
use crate::openjph::ojph_defs::{Si32, Si64, Ui8, Ui32, ojph_max};
use core::ffi::c_void;
use core::ptr;
use std::alloc::{Layout, alloc, dealloc, handle_alloc_error};

pub const LFT_UNDEFINED: Ui32 = 0x00;
pub const LFT_BYTE: Ui32 = 0x01;
pub const LFT_16BIT: Ui32 = 0x02;
pub const LFT_32BIT: Ui32 = 0x04;
pub const LFT_64BIT: Ui32 = 0x08;
pub const LFT_INTEGER: Ui32 = 0x10;
pub const LFT_SIZE_MASK: Ui32 = 0x0F;

#[repr(C)]
#[derive(Clone, Copy)]
pub union LineBufUnion {
    pub i32_: *mut Si32,
    pub i64_: *mut Si64,
    pub f32_: *mut f32,
    pub p: *mut c_void,
}

#[repr(C)]
pub struct LineBuf {
    pub size: usize,
    pub pre_size: Ui32,
    pub flags: Ui32,
    pub storage: LineBufUnion,
}

impl Default for LineBuf {
    fn default() -> Self {
        Self {
            size: 0,
            pre_size: 0,
            flags: LFT_UNDEFINED,
            storage: LineBufUnion {
                i32_: core::ptr::null_mut(),
            },
        }
    }
}

impl LineBuf {
    /// Read-only view of the `i32` data stored in this line buffer.
    ///
    /// # Safety
    /// Caller must ensure `storage.i32_` was initialised by OpenJPH and the
    /// buffer has at least `self.size` elements.
    #[inline]
    pub unsafe fn i32_slice(&self) -> &[Si32] {
        core::slice::from_raw_parts(self.storage.i32_, self.size)
    }

    /// Mutable view of the `i32` data stored in this line buffer.
    ///
    /// # Safety
    /// Same as `i32_slice`, additionally requires exclusive access.
    #[inline]
    pub unsafe fn i32_slice_mut(&mut self) -> &mut [Si32] {
        core::slice::from_raw_parts_mut(self.storage.i32_, self.size)
    }

    /// Safe read-only accessor for the first `len` `i32` elements.
    ///
    /// OpenJPH always initialises the buffer before handing back the pointer,
    /// so null/bounds panics should never trigger in correct usage.
    #[inline]
    pub fn get_i32(&self, len: usize) -> &[Si32] {
        assert!(len <= self.size, "LineBuf::get_i32 length out of range");
        // SAFETY: OpenJPH guarantees the pointer is valid with at least `size` elements.
        unsafe {
            assert!(
                !self.storage.i32_.is_null(),
                "LineBuf::get_i32: storage is null"
            );
            core::slice::from_raw_parts(self.storage.i32_, len)
        }
    }

    /// Safe mutable accessor for the first `len` `i32` elements.
    #[inline]
    pub fn get_i32_mut(&mut self, len: usize) -> &mut [Si32] {
        assert!(len <= self.size, "LineBuf::get_i32_mut length out of range");
        // SAFETY: OpenJPH guarantees the pointer is valid with at least `size` elements.
        unsafe {
            assert!(
                !self.storage.i32_.is_null(),
                "LineBuf::get_i32_mut: storage is null"
            );
            core::slice::from_raw_parts_mut(self.storage.i32_, len)
        }
    }

    /// Point this line at `num_ele` externally owned `Si32` elements.
    ///
    /// # Safety
    /// `buffer` must be valid for reads and writes of `num_ele` elements for as long as the
    /// line is accessed (`get_i32*` and the codec read it through safe slices).
    #[inline]
    pub unsafe fn wrap_i32(&mut self, buffer: *mut Si32, num_ele: usize, pre_size: Ui32) {
        self.storage.i32_ = buffer;
        self.size = num_ele;
        self.pre_size = pre_size;
        self.flags = LFT_32BIT | LFT_INTEGER;
    }

    /// Point this line at `num_ele` externally owned `f32` elements.
    ///
    /// # Safety
    /// `buffer` must be valid for reads and writes of `num_ele` elements for as long as the
    /// line is accessed (`get_i32*` and the codec read it through safe slices).
    #[inline]
    pub unsafe fn wrap_f32(&mut self, buffer: *mut f32, num_ele: usize, pre_size: Ui32) {
        self.storage.f32_ = buffer;
        self.size = num_ele;
        self.pre_size = pre_size;
        self.flags = LFT_32BIT;
    }

    /// Point this line at `num_ele` externally owned `Si64` elements.
    ///
    /// # Safety
    /// `buffer` must be valid for reads and writes of `num_ele` elements for as long as the
    /// line is accessed (`get_i32*` and the codec read it through safe slices).
    #[inline]
    pub unsafe fn wrap_i64(&mut self, buffer: *mut Si64, num_ele: usize, pre_size: Ui32) {
        self.storage.i64_ = buffer;
        self.size = num_ele;
        self.pre_size = pre_size;
        self.flags = LFT_64BIT | LFT_INTEGER;
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct LiftingBuf {
    pub line: *mut LineBuf,
    pub active: bool,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CodedLists {
    pub next_list: *mut CodedLists,
    pub buf_size: Ui32,
    pub avail_size: Ui32,
    pub buf: *mut Ui8,
}

impl CodedLists {
    #[inline]
    pub fn new(size: Ui32, buf: *mut Ui8) -> Self {
        Self {
            next_list: core::ptr::null_mut(),
            buf_size: size,
            avail_size: size,
            buf,
        }
    }
}

#[allow(non_camel_case_types)]
pub struct mem_fixed_allocator {
    storage: Vec<Ui8>,
    store: *mut c_void,
    avail_data: *mut c_void,
    avail_obj: *mut c_void,
    size_data: usize,
    size_obj: usize,
    avail_size_obj: usize,
    avail_size_data: usize,
    allocated_data: usize,
    preallocation: bool,
}

impl Default for mem_fixed_allocator {
    fn default() -> Self {
        let mut allocator = Self {
            storage: Vec::new(),
            store: core::ptr::null_mut(),
            avail_data: core::ptr::null_mut(),
            avail_obj: core::ptr::null_mut(),
            size_data: 0,
            size_obj: 0,
            avail_size_obj: 0,
            avail_size_data: 0,
            allocated_data: 0,
            preallocation: true,
        };
        allocator.restart();
        allocator
    }
}

impl mem_fixed_allocator {
    #[inline]
    pub fn pre_alloc_data<T>(&mut self, num_ele: usize, pre_size: Ui32) {
        debug_assert!(self.preallocation);
        Self::pre_alloc_local::<T, BYTE_ALIGNMENT>(num_ele, pre_size, &mut self.size_data);
    }

    #[inline]
    pub fn pre_alloc_obj<T>(&mut self, num_ele: usize) {
        debug_assert!(self.preallocation);
        Self::pre_alloc_local::<T, OBJECT_ALIGNMENT>(num_ele, 0, &mut self.size_obj);
    }

    pub fn alloc(&mut self) {
        debug_assert!(self.preallocation);
        if self.size_data + self.size_obj > self.allocated_data {
            self.allocated_data = self.size_data + self.size_obj;
            self.allocated_data += (self.allocated_data + 19) / 20;
            self.storage = vec![0; self.allocated_data];
            self.store = self.storage.as_mut_ptr() as *mut c_void;
        }

        self.avail_obj = self.store;
        self.avail_data = if self.store.is_null() {
            core::ptr::null_mut()
        } else {
            unsafe { (self.store as *mut Ui8).add(self.size_obj) as *mut c_void }
        };
        self.avail_size_obj = self.size_obj;
        self.avail_size_data = self.size_data;
        self.preallocation = false;
    }

    #[inline]
    pub fn restart(&mut self) {
        self.avail_obj = core::ptr::null_mut();
        self.avail_data = core::ptr::null_mut();
        self.avail_size_obj = 0;
        self.avail_size_data = 0;
        self.size_obj = 0;
        self.size_data = 0;
        self.preallocation = true;
    }

    #[inline]
    pub fn post_alloc_data<T>(&mut self, num_ele: usize, pre_size: Ui32) -> *mut T {
        Self::post_alloc_local::<T, BYTE_ALIGNMENT>(
            num_ele,
            pre_size,
            self.preallocation,
            &mut self.avail_size_data,
            &mut self.avail_data,
        )
    }

    #[inline]
    pub fn post_alloc_obj<T>(&mut self, num_ele: usize) -> *mut T {
        Self::post_alloc_local::<T, OBJECT_ALIGNMENT>(
            num_ele,
            0,
            self.preallocation,
            &mut self.avail_size_obj,
            &mut self.avail_obj,
        )
    }

    fn pre_alloc_local<T, const N: u32>(num_ele: usize, pre_size: Ui32, sz: &mut usize) {
        let num_ele = calc_aligned_size::<T, N>(num_ele);
        let mut total = (num_ele + pre_size as usize) * core::mem::size_of::<T>();
        total += 2 * N as usize - 1;
        *sz += total;
    }

    fn post_alloc_local<T, const N: u32>(
        num_ele: usize,
        pre_size: Ui32,
        preallocation: bool,
        avail_sz: &mut usize,
        avail_p: &mut *mut c_void,
    ) -> *mut T {
        debug_assert!(!preallocation);
        let num_ele = calc_aligned_size::<T, N>(num_ele);
        let mut total = (num_ele + pre_size as usize) * core::mem::size_of::<T>();
        total += 2 * N as usize - 1;

        let p = unsafe { align_ptr::<T, N>((*avail_p as *mut T).add(pre_size as usize)) };
        *avail_p = unsafe { (*avail_p as *mut Ui8).add(total) as *mut c_void };
        debug_assert!(*avail_sz >= total);
        *avail_sz -= total;
        p
    }
}

pub unsafe fn ojph_aligned_malloc(alignment: usize, size: usize) -> *mut c_void {
    debug_assert!(alignment != 0 && (alignment & (alignment - 1)) == 0);

    let header_size = 2 * core::mem::size_of::<usize>();
    // An unsatisfiable size (overflow, or above `isize::MAX`) is an ordinary allocation failure like
    // `malloc` returning NULL, not a logic error: callers (e.g. `MemOutfile::open`) report it.
    let Some(alloc_size) = size.checked_add(alignment).and_then(|s| s.checked_add(header_size)) else {
        return core::ptr::null_mut();
    };
    let Ok(layout) = Layout::from_size_align(alloc_size, core::mem::align_of::<usize>()) else {
        return core::ptr::null_mut();
    };
    let orig_ptr = unsafe { alloc(layout) };
    if orig_ptr.is_null() {
        return core::ptr::null_mut();
    }

    let start_of_mem = unsafe { orig_ptr.add(header_size) } as usize;
    let aligned_addr = (start_of_mem + alignment - 1) & !(alignment - 1);
    let header = (aligned_addr as *mut usize).wrapping_sub(2);
    unsafe {
        header.write(orig_ptr as usize);
        header.add(1).write(alloc_size);
    }

    aligned_addr as *mut c_void
}

pub unsafe fn ojph_aligned_free(pointer: *mut c_void) {
    if pointer.is_null() {
        return;
    }

    let header = (pointer as *mut usize).wrapping_sub(2);
    let orig_ptr = unsafe { header.read() } as *mut u8;
    let alloc_size = unsafe { header.add(1).read() };
    let layout = match Layout::from_size_align(alloc_size, core::mem::align_of::<usize>()) {
        Ok(layout) => layout,
        Err(_) => {
            debug_assert!(false, "valid aligned allocation layout");
            return;
        }
    };
    unsafe {
        dealloc(orig_ptr, layout);
    }
}

#[allow(non_camel_case_types)]
pub struct mem_elastic_allocator {
    store: *mut stores_list,
    cur_store: *mut stores_list,
    avail: *mut stores_list,
    total_allocated: usize,
    chunk_size: Ui32,
}

#[repr(C)]
struct stores_list {
    next_store: *mut stores_list,
    orig_data: *mut Ui8,
    data: *mut Ui8,
    orig_size: Ui32,
    available: Ui32,
}

impl stores_list {
    /// `sizeof(stores_list)` rounded up to the next multiple of 16.
    ///
    /// Port of OpenJPH `stores_list::stores_list_size16()`
    /// (`ojph_mem.h:253-256`). The payload (the `CodedLists` header plus its
    /// bitstream) must start at a 16-byte boundary so `CodedLists::buf` never
    /// lands at `4 mod 8` (which would misalign the 8-byte pointers inside
    /// `CodedLists`). Rounding the header size to 16 here makes `data` aligned
    /// *by construction*, exactly like the C++ original — so `get_buffer` needs
    /// no run-time alignment padding (the absence of that rounding in the
    /// earlier port is what corrupted the `available` accounting and caused the
    /// HTJ2K encode access-violation on high-byte-count code-blocks).
    #[inline]
    const fn header_size16() -> usize {
        (core::mem::size_of::<stores_list>() + 15) & !15
    }
}

impl stores_list {
    unsafe fn create(available_bytes: Ui32) -> *mut stores_list {
        let store_bytes = Self::eval_store_bytes(available_bytes) as usize;
        let layout =
            match Layout::from_size_align(store_bytes, core::mem::align_of::<stores_list>()) {
                Ok(layout) => layout,
                Err(_) => {
                    debug_assert!(false, "valid stores_list layout");
                    return ptr::null_mut();
                }
            };
        let raw = unsafe { alloc(layout) } as *mut stores_list;
        if raw.is_null() {
            handle_alloc_error(layout);
        }

        // Payload starts after the 16-rounded header so `data` is 16-byte
        // aligned (port of C++ `orig_data = data = (ui8*)this + stores_list_size16()`).
        let data = unsafe { (raw as *mut Ui8).add(Self::header_size16()) };
        unsafe {
            (*raw).next_store = ptr::null_mut();
            (*raw).orig_data = data;
            (*raw).data = data;
            (*raw).orig_size = available_bytes;
            (*raw).available = available_bytes;
        }
        raw
    }

    unsafe fn destroy(node: *mut stores_list) {
        let store_bytes = unsafe { Self::eval_store_bytes((*node).orig_size) as usize };
        let layout =
            match Layout::from_size_align(store_bytes, core::mem::align_of::<stores_list>()) {
                Ok(layout) => layout,
                Err(_) => {
                    debug_assert!(false, "valid stores_list layout");
                    return;
                }
            };
        unsafe {
            dealloc(node as *mut Ui8, layout);
        }
    }

    unsafe fn restart(&mut self) {
        self.next_store = ptr::null_mut();
        self.available = self.orig_size;
        self.data = self.orig_data;
    }

    const fn eval_store_bytes(available_bytes: Ui32) -> Ui32 {
        // Allocate the 16-rounded header (`header_size16`) plus the payload,
        // matching C++ `eval_store_bytes` (`available_bytes + stores_list_size16()`).
        available_bytes + Self::header_size16() as Ui32
    }
}

impl mem_elastic_allocator {
    pub fn new(chunk_size: Ui32) -> Self {
        Self {
            store: ptr::null_mut(),
            cur_store: ptr::null_mut(),
            avail: ptr::null_mut(),
            total_allocated: 0,
            chunk_size,
        }
    }

    unsafe fn allocate(
        chunk_size: Ui32,
        avail: &mut *mut stores_list,
        total_allocated: &mut usize,
        list: *mut *mut stores_list,
        extended_bytes: Ui32,
    ) -> *mut stores_list {
        let bytes = ojph_max(extended_bytes, chunk_size);
        if !(*avail).is_null() && unsafe { (**avail).orig_size >= bytes } {
            unsafe {
                *list = *avail;
                *avail = (**avail).next_store;
                (**list).restart();
                *list
            }
        } else {
            let store_bytes = stores_list::eval_store_bytes(bytes);
            let node_ptr = unsafe { stores_list::create(bytes) };
            *total_allocated += store_bytes as usize;
            unsafe {
                *list = node_ptr;
            }
            node_ptr
        }
    }

    pub fn get_buffer(&mut self, needed_bytes: Ui32, p: &mut *mut CodedLists) {
        // Round the (header + payload) size up to a 16-byte multiple so every
        // `CodedLists` stays 16-byte aligned within the store. Combined with the
        // 16-aligned `data` from `stores_list::create` (see `header_size16`),
        // this keeps `data` aligned across successive `get_buffer` calls WITHOUT
        // any run-time padding. Port of OpenJPH `mem_elastic_allocator::get_buffer`
        // (`ojph_mem.cpp:113-130`):
        //   `raw = needed_bytes + sizeof(coded_lists);
        //    extended_bytes = (raw + 15) & ~15;`
        //
        // The earlier port instead added `align_of::<CodedLists>()` and then
        // subtracted a separately-computed run-time `padding` on top of
        // `extended_bytes`. Because the store was only sized to `extended_bytes`,
        // that extra padding could exceed `available` for a large code-block,
        // underflowing the `u32` `available` and walking `data` past the store
        // end — the silent out-of-bounds write that crashed the HTJ2K encoder
        // (frame robat.0041, tile=1024) with a 0xC0000005 access violation.
        let raw = needed_bytes + core::mem::size_of::<CodedLists>() as Ui32;
        let extended_bytes = (raw + 15) & !15u32;

        unsafe {
            if self.store.is_null() {
                let list = &mut self.store as *mut *mut stores_list;
                self.cur_store = Self::allocate(
                    self.chunk_size,
                    &mut self.avail,
                    &mut self.total_allocated,
                    list,
                    extended_bytes,
                );
            } else if (*self.cur_store).available < extended_bytes {
                let tail_next = &mut (*self.cur_store).next_store as *mut *mut stores_list;
                self.cur_store = Self::allocate(
                    self.chunk_size,
                    &mut self.avail,
                    &mut self.total_allocated,
                    tail_next,
                    extended_bytes,
                );
            }

            // `data` is already 16-byte aligned (header rounded to 16, and every
            // prior `extended_bytes` advance was a 16-multiple), so place the
            // `CodedLists` directly — no alignment padding needed.
            let coded = (*self.cur_store).data as *mut CodedLists;
            ptr::write(
                coded,
                CodedLists::new(
                    needed_bytes,
                    (coded as *mut Ui8).add(core::mem::size_of::<CodedLists>()),
                ),
            );

            debug_assert!((*self.cur_store).available >= extended_bytes);
            (*self.cur_store).available -= extended_bytes;
            (*self.cur_store).data = (*self.cur_store).data.add(extended_bytes as usize);
            *p = coded;
        }
    }

    pub fn restart(&mut self) {
        unsafe {
            let mut p = &mut self.avail as *mut *mut stores_list;
            while !(*p).is_null() {
                p = &mut (**p).next_store as *mut *mut stores_list;
            }
            *p = self.store;
            self.cur_store = ptr::null_mut();
            self.store = ptr::null_mut();
        }
    }
}

impl Drop for mem_elastic_allocator {
    fn drop(&mut self) {
        unsafe {
            while !self.store.is_null() {
                let t = (*self.store).next_store;
                stores_list::destroy(self.store);
                self.store = t;
            }
            while !self.avail.is_null() {
                let t = (*self.avail).next_store;
                stores_list::destroy(self.avail);
                self.avail = t;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CodedLists, LFT_32BIT, LFT_64BIT, LFT_INTEGER, LFT_UNDEFINED, LiftingBuf, LineBuf,
        mem_elastic_allocator, mem_fixed_allocator, ojph_aligned_free, ojph_aligned_malloc,
    };
    use core::ffi::c_void;

    #[test]
    fn default_line_buf_matches_reference_constructor() {
        let line = LineBuf::default();
        assert_eq!(line.size, 0);
        assert_eq!(line.pre_size, 0);
        assert_eq!(line.flags, LFT_UNDEFINED);
    }

    #[test]
    fn wrap_i32_matches_cpp_specialization() {
        let mut data = [1i32, 2, 3];
        let mut line = LineBuf::default();
        unsafe { line.wrap_i32(data.as_mut_ptr(), data.len(), 7) };
        assert_eq!(line.size, 3);
        assert_eq!(line.pre_size, 7);
        assert_eq!(line.flags, LFT_32BIT | LFT_INTEGER);
    }

    #[test]
    fn wrap_f32_matches_cpp_specialization() {
        let mut data = [1.0f32, 2.0, 3.0];
        let mut line = LineBuf::default();
        unsafe { line.wrap_f32(data.as_mut_ptr(), data.len(), 2) };
        assert_eq!(line.flags, LFT_32BIT);
    }

    #[test]
    fn wrap_i64_matches_cpp_specialization() {
        let mut data = [1i64, 2, 3];
        let mut line = LineBuf::default();
        unsafe { line.wrap_i64(data.as_mut_ptr(), data.len(), 4) };
        assert_eq!(line.flags, LFT_64BIT | LFT_INTEGER);
    }

    #[test]
    fn lifting_buf_defaults_match_reference() {
        let lifting = LiftingBuf::default();
        assert!(lifting.line.is_null());
        assert!(!lifting.active);
    }

    #[test]
    fn coded_lists_constructor_matches_reference_layout() {
        let mut buf = [0u8; 8];
        let list = CodedLists::new(8, buf.as_mut_ptr());
        assert!(list.next_list.is_null());
        assert_eq!(list.buf_size, 8);
        assert_eq!(list.avail_size, 8);
        assert_eq!(list.buf, buf.as_mut_ptr());
    }

    #[test]
    fn fixed_allocator_preserves_object_then_data_layout() {
        let mut allocator = mem_fixed_allocator::default();
        allocator.pre_alloc_obj::<u32>(2);
        allocator.pre_alloc_data::<u8>(16, 0);
        allocator.alloc();

        let obj = allocator.post_alloc_obj::<u32>(2);
        let data = allocator.post_alloc_data::<u8>(16, 0);
        assert_eq!((obj as usize) % 8, 0);
        assert_eq!((data as usize) % 64, 0);
        assert!((obj as usize) < (data as usize));
    }

    #[test]
    fn aligned_malloc_and_free_match_reference_alignment_contract() {
        let ptr = unsafe { ojph_aligned_malloc(64, 128) } as *mut u8;
        assert!(!ptr.is_null());
        assert_eq!((ptr as usize) % 64, 0);
        unsafe {
            ojph_aligned_free(ptr as *mut c_void);
        }
    }

    #[test]
    fn elastic_allocator_get_buffer_matches_reference_chunking_shape() {
        let mut allocator = mem_elastic_allocator::new(64);
        let mut list = core::ptr::null_mut();
        allocator.get_buffer(12, &mut list);
        assert!(!list.is_null());
        unsafe {
            assert_eq!((*list).buf_size, 12);
            assert_eq!((*list).avail_size, 12);
            assert!(!(*list).buf.is_null());
        }
    }

    #[test]
    fn elastic_allocator_restart_recycles_previous_store_chain() {
        let mut allocator = mem_elastic_allocator::new(32);
        let mut list_a = core::ptr::null_mut();
        let mut list_b = core::ptr::null_mut();
        allocator.get_buffer(8, &mut list_a);
        allocator.get_buffer(40, &mut list_b);
        allocator.restart();
        let mut list_c = core::ptr::null_mut();
        allocator.get_buffer(8, &mut list_c);
        assert!(!list_c.is_null());
    }
}
