//! Private game-bind relay through SA:MP Input::Send (server-command RPC 50).
//! The default command callback is chat RPC 101 and must never receive a token.
//! RVAs/thiscall ABI are verified against the supported DLLs in the accompanying
//! command-relay evidence. No chat, editbox or input-history storage is used.

use std::ffi::{c_char, c_void};
use std::time::Duration;

const MODULE_WAIT_LIMIT: Duration = Duration::from_secs(10);
const MODULE_POLL_INTERVAL: Duration = Duration::from_millis(50);

fn wait_for_module(
    mut is_loaded: impl FnMut() -> bool,
    mut elapsed: impl FnMut() -> Duration,
    mut sleep: impl FnMut(Duration),
) -> bool {
    loop {
        let elapsed = elapsed();
        if elapsed > MODULE_WAIT_LIMIT { return false; }
        if is_loaded() { return true; }
        let remaining = MODULE_WAIT_LIMIT.saturating_sub(elapsed);
        if remaining.is_zero() { return false; }
        sleep(remaining.min(MODULE_POLL_INTERVAL));
    }
}

/// The ASI loader already calls initialize from its worker, outside DllMain.
/// OMP launcher starts GTA before injecting samp.dll, so absence may be brief.
/// No hooks or version-dependent game pointers are accessed while waiting.
#[cfg(not(test))]
pub fn wait_for_game_module() -> bool {
    let started = std::time::Instant::now();
    wait_for_module(client_api::samp::is_loaded, || started.elapsed(), std::thread::sleep)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameVersion { Dl, R1, R3, Unsupported }

type SendCommand = unsafe extern "thiscall" fn(*mut c_void, *const c_char);

fn send_rva(version: GameVersion) -> Option<usize> {
    match version {
        GameVersion::Dl => Some(0x69340),
        GameVersion::R1 => Some(0x65C60),
        GameVersion::R3 => Some(0x69190),
        GameVersion::Unsupported => None,
    }
}

fn send_address(version: GameVersion, module: usize) -> Option<usize> {
    if module == 0 { return None; }
    module.checked_add(send_rva(version)?)
}

fn bind_command(token: &str) -> Option<[u8; 74]> {
    if token.len() != 64 || !token.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
        return None;
    }
    let mut command = [0u8; 74];
    command[..9].copy_from_slice(b"/__lrcef ");
    command[9..73].copy_from_slice(token.as_bytes());
    Some(command)
}

fn expected_prologue(bytes: &[u8]) -> bool {
    // The following SEH addresses are relocated and differ in R3-1. Only this
    // common, relocation-free prefix is compared; absolute file bytes are not.
    bytes == [0x64, 0xa1, 0, 0, 0, 0, 0x6a, 0xff, 0x68]
}

unsafe fn invoke(input: *mut c_void, send: SendCommand, command: &[u8; 74]) -> bool {
    if input.is_null() { return false; }
    unsafe { send(input, command.as_ptr().cast()); }
    true
}

#[cfg(not(test))]
fn loaded_version() -> Option<GameVersion> {
    use client_api::samp::version::{version, Version};
    if !client_api::samp::is_loaded() { return None; }
    Some(match version() {
        Version::V03DL => GameVersion::Dl,
        Version::V037 => GameVersion::R1,
        Version::V037R3 => GameVersion::R3,
        _ => GameVersion::Unsupported,
    })
}

#[cfg(not(test))]
pub fn supported_game() -> bool { loaded_version().and_then(send_rva).is_some() }

#[cfg(not(test))]
fn executable_send(module: usize, address: usize) -> bool {
    use winapi::um::memoryapi::VirtualQuery;
    use winapi::um::winnt::{MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_EXECUTE_READ,
        PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY, PAGE_GUARD};
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<MEMORY_BASIC_INFORMATION>();
    if unsafe { VirtualQuery(address as *const winapi::ctypes::c_void, &mut info, size) } != size {
        return false;
    }
    let readable_code = PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY;
    let Some(end) = address.checked_add(9) else { return false; };
    let Some(region_end) = (info.BaseAddress as usize).checked_add(info.RegionSize) else { return false; };
    if info.State != MEM_COMMIT || info.AllocationBase as usize != module
        || info.Protect & PAGE_GUARD != 0 || info.Protect & readable_code == 0 || end > region_end {
        return false;
    }
    expected_prologue(unsafe { std::slice::from_raw_parts(address as *const u8, 9) })
}

/// Called only on the existing game thread after pinned QUIC delivers a token.
#[cfg(not(test))]
pub fn send_binding(token: &str) -> bool {
    let Some(command) = bind_command(token) else { return false; };
    let Some(version) = loaded_version() else { return false; };
    let module = client_api::samp::handle() as usize;
    let Some(address) = send_address(version, module) else { return false; };
    if !executable_send(module, address) { return false; }
    let Some(input) = client_api::samp::inputs::Input::get() else { return false; };
    let input = (input as *mut client_api::samp::inputs::Input).cast::<c_void>();
    let send: SendCommand = unsafe { std::mem::transmute(address) };
    unsafe { invoke(input, send, &command) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn loaded_module_does_not_delay_startup() {
        let sleeps = Cell::new(0);
        assert!(wait_for_module(|| true, || Duration::ZERO, |_| sleeps.set(sleeps.get() + 1)));
        assert_eq!(sleeps.get(), 0);
    }

    #[test]
    fn delayed_module_is_observed_before_initialization() {
        let clock = Cell::new(Duration::ZERO);
        let sleeps = Cell::new(0);
        assert!(wait_for_module(|| clock.get() >= Duration::from_millis(125), || clock.get(), |d| {
            clock.set(clock.get() + d);
            sleeps.set(sleeps.get() + 1);
        }));
        assert_eq!(clock.get(), Duration::from_millis(150));
        assert_eq!(sleeps.get(), 3);
    }

    #[test]
    fn missing_module_times_out_and_late_load_is_not_accepted() {
        let clock = Cell::new(Duration::ZERO);
        let polls = Cell::new(0);
        assert!(!wait_for_module(|| { polls.set(polls.get() + 1); false }, || clock.get(), |d| {
            assert!(d <= MODULE_POLL_INTERVAL);
            clock.set(clock.get() + d);
        }));
        assert_eq!(clock.get(), MODULE_WAIT_LIMIT);
        assert_eq!(polls.get(), 201);
        assert!(!wait_for_module(|| true, || MODULE_WAIT_LIMIT + Duration::from_millis(1), |_| panic!("past deadline")));
    }

    #[test]
    fn only_exact_private_command_can_be_built() {
        let token = "0123456789abcdef".repeat(4);
        let command = bind_command(&token).unwrap();
        assert_eq!(&command[..9], b"/__lrcef ");
        assert_eq!(&command[9..73], token.as_bytes());
        assert_eq!(command[73], 0);
        for bad in ["".into(), "a".repeat(63), "a".repeat(65), "A".repeat(64),
            "g".repeat(64), "\0".repeat(64), " ".repeat(64), "é".repeat(32)] {
            assert!(bind_command(&bad).is_none());
        }
    }

    #[test]
    fn routes_only_verified_versions_and_loaded_modules() {
        assert_eq!(send_address(GameVersion::Dl, 0x10000000), Some(0x10069340));
        assert_eq!(send_address(GameVersion::R1, 0x10000000), Some(0x10065c60));
        assert_eq!(send_address(GameVersion::R3, 0x10000000), Some(0x10069190));
        assert_eq!(send_address(GameVersion::Unsupported, 0x10000000), None);
        assert_eq!(send_address(GameVersion::Dl, 0), None);
        assert_eq!(send_address(GameVersion::Dl, usize::MAX), None);
        assert!(expected_prologue(&[0x64, 0xa1, 0, 0, 0, 0, 0x6a, 0xff, 0x68]));
        assert!(!expected_prologue(&[0xe9, 0, 0, 0, 0, 0, 0, 0, 0]));
    }

    // This naked Win32 stub observes the real call ABI: ECX is Input*, the
    // sole stack argument is the C string, and callee removes exactly 4 bytes.
    #[unsafe(naked)]
    unsafe extern "thiscall" fn observe_abi(_: *mut c_void, _: *const c_char) {
        std::arch::naked_asm!("mov eax, [esp + 4]", "mov [ecx], eax", "ret 4");
    }

    #[test]
    fn real_i686_thiscall_uses_ecx_and_one_stack_argument_without_buffer_changes() {
        #[repr(C)]
        struct Receiver { argument: *const c_char, untouched: [u8; 1024] }
        let command = bind_command(&"abcdef0123456789".repeat(4)).unwrap();
        let mut receiver = Receiver { argument: std::ptr::null(), untouched: [0x5a; 1024] };
        for _ in 0..10_000 {
            assert!(unsafe { invoke((&mut receiver as *mut Receiver).cast(), observe_abi, &command) });
            assert_eq!(receiver.argument, command.as_ptr().cast());
            assert_eq!(receiver.untouched, [0x5a; 1024]);
        }
        assert!(!unsafe { invoke(std::ptr::null_mut(), observe_abi, &command) });
        assert_eq!(std::ffi::CStr::from_bytes_with_nul(&command).unwrap().to_bytes(), &command[..73]);
    }
}
