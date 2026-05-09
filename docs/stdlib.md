# Pandora Standard Library

This document describes the current public standard library surface from `stdlib/std/*.pand`.

## Overview

Pandora currently exposes 24 stdlib modules:

- `std/cli`
- `std/core`
- `std/crypto`
- `std/csv`
- `std/encoding`
- `std/env`
- `std/fs`
- `std/http`
- `std/io`
- `std/json`
- `std/log`
- `std/math`
- `std/mime`
- `std/net`
- `std/os`
- `std/path`
- `std/proc`
- `std/rand`
- `std/regex`
- `std/sync`
- `std/thread`
- `std/time`
- `std/url`
- `std/xml`

## Category Map

### I/O

- `std/io`
- `std/fs`
- `std/path`
- `std/http`
- `std/net`

### Strings / Text / Encoding

- `std/encoding`
- `std/regex`
- `std/mime`
- `std/url`
- `std/json`
- `std/xml`
- `std/csv`

### Collections and Data-Shaping Helpers

- `std/core` (array/bool/string helper utilities)
- Data containers appear across modules via `map[...]`, `set[...]`, tuples, arrays, and `any` payloads.

### Utility / System Functions

- `std/math`
- `std/time`
- `std/rand`
- `std/crypto`
- `std/log`
- `std/env`
- `std/cli`
- `std/os`
- `std/proc`
- `std/thread`
- `std/sync`

## Complete API Reference

## std/cli

- `fn args_cli() -> [str]`
- `fn arg_count() -> u64`
- `fn positional() -> [str]`
- `fn command() -> (str, err)`
- `fn has_flag(flag: str) -> bool`
- `fn flag_value(flag: str) -> (str, err)`
- `fn help_requested() -> bool`
- `fn version_requested() -> bool`

## std/core

- `fn helper() -> str`
- `fn assert(cond: bool, message: str = "assertion failed") -> unit`
- `fn assert_eq_i32(a: i32, b: i32, message: str = "i32 values differ") -> unit`
- `fn assert_eq_str(a: str, b: str, message: str = "str values differ") -> unit`
- `fn str_repeat(s: str, times: i32) -> str`
- `fn str_pad_left(s: str, total: i32, pad: str = " ") -> str`
- `fn str_pad_right(s: str, total: i32, pad: str = " ") -> str`
- `fn sum_i32(arr: [i32]) -> i32`
- `fn count_true(arr: [bool]) -> i32`

## std/crypto

- `fn sha256(input: str) -> str`
- `fn random_bytes(len: u64) -> (str, err)`
- `fn random_u64() -> (u64, err)`
- `fn encrypt(plaintext: str, key: str) -> (str, err)`
- `fn decrypt(ciphertext: str, key: str) -> (str, err)`

## std/csv

- `fn parse_csv(raw: str) -> (any, err)`
- `fn stringify_csv(rows: any) -> (str, err)`

## std/encoding

- `fn base64_encode(input: str) -> str`
- `fn base64_decode(input: str) -> (str, err)`
- `fn hex_encode(input: str) -> str`
- `fn hex_decode(input: str) -> (str, err)`
- `fn is_ascii(input: str) -> bool`
- `fn ascii_upper(input: str) -> str`
- `fn ascii_lower(input: str) -> str`
- `fn utf8_len(input: str) -> u64`
- `fn utf8_is_valid(input: str) -> bool`

## std/env

- `fn get_env(key: str) -> (str, err)`
- `fn get_env_or(key: str, default: str) -> str`
- `fn set_env(key: str, value: str) -> err`
- `fn unset_env(key: str) -> err`
- `fn has_env(key: str) -> bool`
- `fn list_env() -> map[str]str`
- `fn list_env_prefix(prefix: str) -> map[str]str`

## std/fs

- `fn exists(path: str) -> bool`
- `fn is_file(path: str) -> bool`
- `fn is_dir(path: str) -> bool`
- `fn create_dir(path: str) -> err`
- `fn create_dir_all(path: str) -> err`
- `fn read_dir(path: str) -> ([str], err)`
- `fn remove_file(path: str) -> err`
- `fn remove_dir(path: str) -> err`
- `fn remove_dir_all(path: str) -> err`
- `fn rename(from: str, to: str) -> err`
- `fn copy(from: str, to: str) -> (u64, err)`
- `fn cwd() -> (str, err)`
- `fn set_cwd(path: str) -> err`
- `fn path_join(a: str, b: str) -> str`
- `fn path_parent(path: str) -> (str, err)`
- `fn path_filename(path: str) -> (str, err)`
- `fn path_extension(path: str) -> (str, err)`
- `fn metadata_len(path: str) -> (u64, err)`
- `fn metadata_readonly(path: str) -> (bool, err)`
- `fn set_readonly(path: str, readonly: bool) -> err`

## std/http

- `fn parse_headers(raw: str) -> (map[str]str, err)`
- `fn parse_response(raw: str) -> (i32, str, map[str]str, str, err)`
- `fn parse_request(raw: str) -> (str, str, str, map[str]str, str, err)`
- `fn get(url: str) -> (i32, str, map[str]str, str, err)`
- `fn listen(addr: str) -> (u64, err)`
- `fn local_addr(id: u64) -> (str, err)`
- `fn respond_once(id: u64, response: str) -> err`

## std/io

- `fn stdout_write(text: str) -> unit`
- `fn stdout_writeln(text: str) -> unit`
- `fn stderr_write(text: str) -> unit`
- `fn stderr_writeln(text: str) -> unit`
- `fn stdin_read_line() -> (str, err)`
- `fn read_text(path: str) -> (str, err)`
- `fn write_text(path: str, content: str) -> err`
- `fn append_text(path: str, content: str) -> err`
- `fn exists(path: str) -> bool`
- `fn remove_file(path: str) -> err`
- `fn buffer_new() -> str`
- `fn buffer_write(buffer: str, chunk: str) -> str`
- `fn buffer_writeln(buffer: str, line: str) -> str`
- `fn buffer_clear() -> str`

## std/json

- `fn parse_json(raw: str) -> (any, err)`
- `fn stringify_json(value: any) -> (str, err)`
- `fn stringify_json_pretty(value: any) -> (str, err)`

## std/log

- `fn set_log_level(level: str) -> err`
- `fn set_log_prefix(prefix: str) -> err`
- `fn set_log_json(enabled: bool) -> err`
- `fn write_log(level: str, message: str) -> err`
- `fn trace(message: str) -> err`
- `fn debug(message: str) -> err`
- `fn info(message: str) -> err`
- `fn warn(message: str) -> err`
- `fn error_msg(message: str) -> err`

## std/math

- `fn pi() -> f64`
- `fn e() -> f64`
- `fn tau() -> f64`
- `fn abs(x: f64) -> f64`
- `fn sqrt(x: f64) -> f64`
- `fn pow(base: f64, exp: f64) -> f64`
- `fn exp(x: f64) -> f64`
- `fn log(x: f64) -> f64`
- `fn log10(x: f64) -> f64`
- `fn floor(x: f64) -> f64`
- `fn ceil(x: f64) -> f64`
- `fn round(x: f64) -> f64`
- `fn trunc(x: f64) -> f64`
- `fn fract(x: f64) -> f64`
- `fn sin(x: f64) -> f64`
- `fn cos(x: f64) -> f64`
- `fn tan(x: f64) -> f64`
- `fn asin(x: f64) -> f64`
- `fn acos(x: f64) -> f64`
- `fn atan(x: f64) -> f64`
- `fn rand_f64() -> f64`
- `fn rand_i32(min: i32, max: i32) -> (i32, err)`

## std/mime

- `fn guess_mime(path: str) -> str`
- `fn from_extension(ext: str) -> str`
- `fn is_text_mime(mime: str) -> bool`

## std/net

- `fn dns_lookup(host: str) -> ([str], err)`
- `fn udp_bind(addr: str) -> (u64, err)`
- `fn udp_local_addr(id: u64) -> (str, err)`
- `fn udp_send_to(id: u64, payload: str, to: str) -> (u64, err)`
- `fn udp_recv_from(id: u64, max: u64) -> (str, str, err)`
- `fn tcp_connect(addr: str) -> (u64, err)`
- `fn tcp_send(id: u64, payload: str) -> (u64, err)`
- `fn tcp_recv(id: u64, max: u64) -> (str, err)`
- `fn tcp_close(id: u64) -> err`

## std/os

- `fn platform() -> str`
- `fn arch() -> str`
- `fn pid() -> u64`
- `fn getenv(key: str) -> (str, err)`
- `fn setenv(key: str, value: str) -> err`
- `fn unsetenv(key: str) -> err`
- `fn args() -> [str]`
- `fn exec(command: str) -> (i32, str, str, err)`
- `fn signal_term() -> i32`
- `fn signal_kill() -> i32`
- `fn signal_int() -> i32`
- `fn send_signal(pid: u64, signal: i32) -> err`

## std/path

- `fn path_join(a: str, b: str) -> str`
- `fn path_normalize(path: str) -> str`
- `fn path_parent(path: str) -> (str, err)`
- `fn path_filename(path: str) -> (str, err)`
- `fn path_extension(path: str) -> (str, err)`
- `fn path_stem(path: str) -> (str, err)`
- `fn path_is_abs(path: str) -> bool`
- `fn path_is_relative(path: str) -> bool`

## std/proc

- `fn spawn(command: str) -> (u64, err)`
- `fn wait(pid: u64) -> (i32, err)`
- `fn kill(pid: u64) -> err`
- `fn exec_proc(command: str) -> (i32, str, str, err)`
- `fn pipe(left: str, right: str) -> (i32, str, str, err)`

## std/rand

- `fn seed(value: u64) -> err`
- `fn next_u64() -> u64`
- `fn next_f64() -> f64`
- `fn range_i32(min: i32, max: i32) -> (i32, err)`
- `fn range_u64(min: u64, max: u64) -> (u64, err)`
- `fn bytes_hex(len: u64) -> (str, err)`

## std/regex

- `fn is_match(pattern: str, input: str) -> (bool, err)`
- `fn find(pattern: str, input: str) -> (str, err)`
- `fn find_all(pattern: str, input: str) -> ([str], err)`
- `fn replace_regex(pattern: str, input: str, replacement: str) -> (str, err)`
- `fn replace_all_regex(pattern: str, input: str, replacement: str) -> (str, err)`

## std/sync

- `fn mutex_new_sync() -> u64`
- `fn mutex_lock_sync(id: u64) -> err`
- `fn mutex_unlock_sync(id: u64) -> err`
- `fn atomic_i64_new(initial: i64) -> u64`
- `fn atomic_i64_load(id: u64) -> (i64, err)`
- `fn atomic_i64_store(id: u64, value: i64) -> err`
- `fn atomic_i64_add(id: u64, delta: i64) -> (i64, err)`
- `fn channel_new() -> u64`
- `fn channel_send(id: u64, value: any) -> err`
- `fn channel_recv(id: u64) -> (any, err)`
- `fn channel_try_recv(id: u64) -> (any, bool, err)`

## std/thread

- `fn spawn_thread(command: str) -> (u64, err)`
- `fn join_thread(tid: u64) -> (i32, err)`
- `fn sleep_thread_ms(ms: u64) -> err`
- `fn mutex_new() -> u64`
- `fn mutex_lock(id: u64) -> err`
- `fn mutex_try_lock(id: u64) -> (bool, err)`
- `fn mutex_unlock(id: u64) -> err`

## std/time

- `fn now_unix_secs() -> u64`
- `fn now_unix_millis() -> u64`
- `fn sleep_ms(ms: u64) -> err`
- `fn tick_ms() -> u64`
- `fn elapsed_ms(start: u64) -> u64`
- `fn now_iso_utc() -> (str, err)`
- `fn from_unix_secs_iso_utc(secs: u64) -> (str, err)`

## std/url

- `fn parse_url(raw: str) -> (map[str]str, err)`
- `fn build_url(scheme: str, host: str, path: str, query: str, fragment: str) -> (str, err)`
- `fn encode_url_component(input: str) -> str`
- `fn decode_url_component(input: str) -> (str, err)`

## std/xml

- `fn parse_xml(raw: str) -> (any, err)`
- `fn stringify_xml(name: str, text: str) -> (str, err)`

## Notes

- This file documents the public wrappers declared in `stdlib/std/*.pand`.
- Many functions delegate to internal runtime intrinsics.
- Public prelude builtins (`print`, `len`, `error`, `panic`, `wrap`, `typeof`) are not part of a `std/*` module export list.
