use std::{
  ffi::{CStr, c_void},
  mem::size_of,
  sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
  },
  time::Duration,
};

use sentry::protocol::{Event, Value};
use windows::{
  Win32::{
    Foundation::{HMODULE, NTSTATUS},
    System::{
      Diagnostics::Debug::{
        AddVectoredExceptionHandler, AddrModeFlat, EXCEPTION_CONTINUE_SEARCH, EXCEPTION_POINTERS, IMAGEHLP_LINE64,
        STACKFRAME64, SYMBOL_INFO, SYMBOL_INFO_PACKAGE, SYMOPT_DEFERRED_LOADS, SYMOPT_LOAD_LINES, SYMOPT_UNDNAME,
        SetUnhandledExceptionFilter, StackWalk64, SymFromAddr, SymFunctionTableAccess64, SymGetLineFromAddr64,
        SymGetModuleBase64, SymInitialize, SymSetOptions,
      },
      LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleFileNameA,
        GetModuleHandleExA,
      },
      Threading::{GetCurrentProcess, GetCurrentThread},
    },
  },
  core::PCSTR,
};

const EXCEPTION_ACCESS_VIOLATION: u32 = 0xC0000005;
const EXCEPTION_ARRAY_BOUNDS_EXCEEDED: u32 = 0xC000008C;
const EXCEPTION_DATATYPE_MISALIGNMENT: u32 = 0x80000002;
const EXCEPTION_FLT_DIVIDE_BY_ZERO: u32 = 0xC000008E;
const EXCEPTION_ILLEGAL_INSTRUCTION: u32 = 0xC000001D;
const EXCEPTION_INT_DIVIDE_BY_ZERO: u32 = 0xC0000094;
const EXCEPTION_IN_PAGE_ERROR: u32 = 0xC0000006;
const EXCEPTION_STACK_OVERFLOW: u32 = 0xC00000FD;
const STATUS_STACK_BUFFER_OVERRUN: u32 = 0xC0000409;
const IMAGE_FILE_MACHINE_AMD64: u32 = 0x8664;
const MAX_STACK_FRAMES: usize = 16;

static SEH_INSTALLED: AtomicBool = AtomicBool::new(false);
static SEH_LOGGING: AtomicBool = AtomicBool::new(false);
static DBGHELP_LOCK: Mutex<()> = Mutex::new(());

pub fn install() {
  if SEH_INSTALLED.swap(true, Ordering::AcqRel) {
    return;
  }

  unsafe {
    AddVectoredExceptionHandler(1, Some(vectored_exception_handler));
    SetUnhandledExceptionFilter(Some(unhandled_exception_filter));
    SymSetOptions(SYMOPT_DEFERRED_LOADS | SYMOPT_LOAD_LINES | SYMOPT_UNDNAME);
    if let Err(error) = SymInitialize(GetCurrentProcess(), PCSTR::null(), true) {
      log_seh(&format!("[seh] rust SEH symbol initialization failed: {error}"));
      return;
    }
  }

  log_seh("[seh] rust windows SEH logger installed");
}

unsafe extern "system" fn vectored_exception_handler(info: *mut EXCEPTION_POINTERS) -> i32 {
  if !is_diagnostic_exception(info) {
    return EXCEPTION_CONTINUE_SEARCH;
  }
  if !SEH_LOGGING.swap(true, Ordering::AcqRel) {
    unsafe {
      log_exception("vectored", info);
    }
  }
  EXCEPTION_CONTINUE_SEARCH
}

unsafe extern "system" fn unhandled_exception_filter(info: *const EXCEPTION_POINTERS) -> i32 {
  unsafe {
    log_exception("unhandled", info.cast_mut());
  }
  EXCEPTION_CONTINUE_SEARCH
}

fn is_diagnostic_exception(info: *mut EXCEPTION_POINTERS) -> bool {
  if info.is_null() {
    return false;
  }

  let record = unsafe { (*info).ExceptionRecord };
  if record.is_null() {
    return false;
  }

  is_diagnostic_code(ntstatus_code(unsafe { (*record).ExceptionCode }))
}

fn is_diagnostic_code(code: u32) -> bool {
  matches!(
    code,
    EXCEPTION_ACCESS_VIOLATION
      | EXCEPTION_ARRAY_BOUNDS_EXCEEDED
      | EXCEPTION_DATATYPE_MISALIGNMENT
      | EXCEPTION_FLT_DIVIDE_BY_ZERO
      | EXCEPTION_ILLEGAL_INSTRUCTION
      | EXCEPTION_INT_DIVIDE_BY_ZERO
      | EXCEPTION_IN_PAGE_ERROR
      | EXCEPTION_STACK_OVERFLOW
      | STATUS_STACK_BUFFER_OVERRUN
  )
}

unsafe fn log_exception(source: &str, info: *mut EXCEPTION_POINTERS) {
  if info.is_null() {
    return;
  }
  let record = unsafe { (*info).ExceptionRecord };
  if record.is_null() {
    return;
  }

  let record = unsafe { &*record };
  let code = ntstatus_code(record.ExceptionCode);
  let location = module_location(record.ExceptionAddress as u64);
  let fault0 = exception_parameter(record, 0);
  let fault1 = exception_parameter(record, 1);
  let fault2 = exception_parameter(record, 2);
  log_seh(&format!(
    "[seh] {source} exception: code=0x{code:08x} kind={} address={:p} module={} fault0=0x{:x} fault1=0x{:x} fault2=0x{:x}",
    seh_code_name(code),
    record.ExceptionAddress,
    location,
    fault0,
    fault1,
    fault2
  ));
  let stack = unsafe { stack_trace(info) };
  if let Some(stack) = stack.as_deref() {
    log_seh(&format!("[seh] stack: {stack}"));
  }
  report_sentry_seh(
    source,
    code,
    record.ExceptionAddress,
    &location,
    fault0,
    fault1,
    fault2,
    stack.as_deref(),
  );
}

unsafe fn stack_trace(info: *mut EXCEPTION_POINTERS) -> Option<String> {
  if info.is_null() {
    return None;
  }
  let context_record = unsafe { (*info).ContextRecord };
  if context_record.is_null() {
    return None;
  }

  #[cfg(target_arch = "x86_64")]
  unsafe {
    let mut context = *context_record;
    let process = GetCurrentProcess();
    let thread = GetCurrentThread();
    let mut frame = STACKFRAME64::default();
    frame.AddrPC.Offset = context.Rip;
    frame.AddrPC.Mode = AddrModeFlat;
    frame.AddrFrame.Offset = context.Rbp;
    frame.AddrFrame.Mode = AddrModeFlat;
    frame.AddrStack.Offset = context.Rsp;
    frame.AddrStack.Mode = AddrModeFlat;

    let _dbghelp_guard = DBGHELP_LOCK.lock().ok();
    let mut parts = Vec::with_capacity(MAX_STACK_FRAMES);
    parts.push(format!(
      "#0=0x{:016x} {}",
      context.Rip,
      symbol_location(process, context.Rip)
    ));

    for index in 1..MAX_STACK_FRAMES {
      if !StackWalk64(
        IMAGE_FILE_MACHINE_AMD64,
        process,
        thread,
        &mut frame,
        &mut context as *mut _ as *mut _,
        None,
        Some(stack_walk_function_table_access),
        Some(stack_walk_module_base),
        None,
      )
      .as_bool()
      {
        break;
      }
      if frame.AddrPC.Offset == 0 {
        break;
      }
      parts.push(format!(
        "#{index}=0x{:016x} {}",
        frame.AddrPC.Offset,
        symbol_location(process, frame.AddrPC.Offset)
      ));
    }

    Some(parts.join(" "))
  }

  #[cfg(not(target_arch = "x86_64"))]
  {
    Some("unsupported Windows architecture for Rust stack walk".to_owned())
  }
}

fn report_sentry_seh(
  source: &str,
  code: u32,
  address: *mut c_void,
  module: &str,
  fault0: usize,
  fault1: usize,
  fault2: usize,
  stack: Option<&str>,
) {
  let sentry_module = sentry_module_location(module);
  let mut event = Event {
    level: sentry::Level::Fatal,
    logger: Some("native::windows::seh".to_owned()),
    message: Some(format!(
      "Windows SEH {source} exception: code=0x{code:08x} kind={} module={sentry_module}",
      seh_code_name(code)
    )),
    ..Default::default()
  };
  event.tags.insert("exception.source".to_owned(), source.to_owned());
  event.tags.insert("exception.code".to_owned(), format!("0x{code:08x}"));
  event
    .tags
    .insert("exception.kind".to_owned(), seh_code_name(code).to_owned());
  event
    .extra
    .insert("exception_address".to_owned(), Value::String(format!("{address:p}")));
  event.extra.insert("module".to_owned(), Value::String(sentry_module));
  event
    .extra
    .insert("fault0".to_owned(), Value::String(format!("0x{fault0:x}")));
  event
    .extra
    .insert("fault1".to_owned(), Value::String(format!("0x{fault1:x}")));
  event
    .extra
    .insert("fault2".to_owned(), Value::String(format!("0x{fault2:x}")));
  if let Some(stack) = stack {
    event.extra.insert("stack".to_owned(), Value::String(stack.to_owned()));
  }

  sentry::capture_event(event);
  let _ = crate::services::logger::flush_sentry(Duration::from_secs(2));
}

fn sentry_module_location(location: &str) -> String {
  let Some((path, offset)) = location.rsplit_once("+0x") else {
    return location.to_owned();
  };
  let file_name = path.rsplit(['\\', '/']).next().unwrap_or(path);
  format!("{file_name}+0x{offset}")
}

unsafe extern "system" fn stack_walk_function_table_access(
  process: windows::Win32::Foundation::HANDLE,
  address: u64,
) -> *mut c_void {
  unsafe { SymFunctionTableAccess64(process, address) }
}

unsafe extern "system" fn stack_walk_module_base(process: windows::Win32::Foundation::HANDLE, address: u64) -> u64 {
  unsafe { SymGetModuleBase64(process, address) }
}

fn symbol_location(process: windows::Win32::Foundation::HANDLE, address: u64) -> String {
  let module = module_location(address);
  let mut text = module;

  let mut package = SYMBOL_INFO_PACKAGE::default();
  package.si.SizeOfStruct = size_of::<SYMBOL_INFO>() as u32;
  package.si.MaxNameLen = package.name.len() as u32;
  let mut displacement = 0u64;
  if unsafe { SymFromAddr(process, address, Some(&mut displacement), &mut package.si) }.is_ok() {
    let name = unsafe { CStr::from_ptr(package.si.Name.as_ptr()) }.to_string_lossy();
    text.push_str(&format!(" {name}+0x{displacement:x}"));
  }

  let mut line = IMAGEHLP_LINE64::default();
  line.SizeOfStruct = size_of::<IMAGEHLP_LINE64>() as u32;
  let mut line_displacement = 0u32;
  if unsafe { SymGetLineFromAddr64(process, address, &mut line_displacement, &mut line) }.is_ok()
    && !line.FileName.0.is_null()
  {
    let file_name = unsafe { CStr::from_ptr(line.FileName.0.cast()) }.to_string_lossy();
    text.push_str(&format!(" {}:{}+0x{:x}", file_name, line.LineNumber, line_displacement));
  }

  text
}

fn module_location(address: u64) -> String {
  let mut module = HMODULE::default();
  let flags = GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
  let module_result = unsafe { GetModuleHandleExA(flags, PCSTR(address as *const u8), &mut module) };
  if module_result.is_err() || module.0.is_null() {
    return "unknown+0x0".to_string();
  }

  let module_base = module.0 as usize as u64;
  let module_rva = address.saturating_sub(module_base);
  let mut path = [0u8; 260];
  let path_len = unsafe { GetModuleFileNameA(Some(module), &mut path) } as usize;
  if path_len == 0 || path_len >= path.len() {
    return format!("module@{:p}+0x{module_rva:x}", module.0);
  }

  let module_path = String::from_utf8_lossy(&path[..path_len]);
  format!("{module_path}+0x{module_rva:x}")
}

fn seh_code_name(code: u32) -> &'static str {
  match code {
    EXCEPTION_ACCESS_VIOLATION => "access violation",
    EXCEPTION_ARRAY_BOUNDS_EXCEEDED => "array bounds exceeded",
    EXCEPTION_DATATYPE_MISALIGNMENT => "datatype misalignment",
    EXCEPTION_FLT_DIVIDE_BY_ZERO => "float divide by zero",
    EXCEPTION_ILLEGAL_INSTRUCTION => "illegal instruction",
    EXCEPTION_INT_DIVIDE_BY_ZERO => "integer divide by zero",
    EXCEPTION_IN_PAGE_ERROR => "in-page error",
    EXCEPTION_STACK_OVERFLOW => "stack overflow",
    STATUS_STACK_BUFFER_OVERRUN => "stack buffer overrun / fail fast",
    _ => "unknown",
  }
}

fn exception_parameter(record: &windows::Win32::System::Diagnostics::Debug::EXCEPTION_RECORD, index: usize) -> usize {
  if record.NumberParameters as usize > index {
    record.ExceptionInformation[index]
  } else {
    0
  }
}

fn ntstatus_code(code: NTSTATUS) -> u32 {
  code.0 as u32
}

fn log_seh(message: &str) {
  tracing::error!(target: "native::windows::seh", "[native/windows/error] {message}");
}
