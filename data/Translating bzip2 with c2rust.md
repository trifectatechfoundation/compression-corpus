+++
title = "Translating bzip2 with c2rust"
slug = "translating-bzip2-with-c2rust"
authors = ["Folkert de Vries"]
date = "2025-03-10"

[taxonomies]
tags = ["bzip2-rs", "data compression"] 

+++

Over the past couple of months we've been hard at work on [libbzip2-rs](https://github.com/trifectatechfoundation/libbzip2-rs), a 100% Rust drop-in compatible implementation the bzip2 compression and decompression functionality.

<!-- more -->

For this project, we used [`c2rust`](https://github.com/immunant/c2rust) for the initial translation from the bzip2 C code to a Rust implementation. The generated Rust code has now been cleaned up and made safe where possible. This post describes our experiences using c2rust for this project.

The work on the project is kindly funded by [NLnet Foundation](https://nlnet.nl/).

## Using c2rust

`c2rust` is an incredibly useful tool developed by [Immunant](https://immunant.com/) and [Galois](https://www.galois.com/). It takes a C code base, and turns it into Rust code. You can try it online at [c2rust.com](https://c2rust.com/).

However, while the output compiles and runs, the code that it generates is full of unsafe and often very C-like, far removed from idiomatic Rust. To create a solid Rust library, cleanup work is required.

We picked c2rust for this project because the [bzip2 C code base](https://gitlab.com/bzip2/bzip2) is relatively straightforward: it's small, does not use threads, and the library is fairly platform-agnostic (e.g. no target-specific SIMD). Using c2rust for bzip2 is a real but manageable project.

## Cleaning up `c2rust` output

One common pattern where the `c2rust` output is unfortunate is simple `for` loops, for instance:

```c
int array[256] = {};
int i;
for (i = 0; i < 256; i++) {
    array[i] = i;
}
```

With the latest release, `c2rust` generates this Rust code:

```rust
let mut array: [libc::c_int; 256] = [0; 256];
let mut i: libc::c_int = 0;
i = 0 as libc::c_int;
while i < 256 as libc::c_int {
    array[i as usize] = i;
    i += 1;
    i;
}
```

(The online version does not yet include my [PR](https://github.com/immunant/c2rust/pull/1170) that expands the zeroed array as `[0; N]` instead of a long literal `[0, 0, 0, ... ]`.)

The code is perfectly functional, and actually completely safe, but it doesn't look right: 

- `i++` is translated as `{ i += 1; i }`, the `; i` part is pointless
- a `for` loop over a range became a `while` loop in Rust
- lots of explicit type annotations and casting

We might want to refactor to something more like this:

```rust
let mut array: [libc::c_int; 256] = [0; 256];
for (i, element) in array.iter_mut().enumerate() { 
    *element = i as libc::c_int;
}
```

or in this specific case even something like this:

```rust
let array: [_; 256] = core::array::from_fn(|i| i as libc::c_int);
```

Currently these refactorings all need to be performed manually.

### Loops

Why is a C `for` loop translated into a Rust `while` loop?

The reason is that the conversion to a `while` loop is always valid, and no sophisticated analysis is needed to check that there are no sneaky mutations to the loop variable. E.g this is valid in C:

```c
int i = 0;
for (; i < 10; i++) {
    if (some_condition) {
        i++;
    }
}
```

This C `for` loop cannot be cleanly translated to a Rust `for` loop over a range, and `c2rust` plays it safe by just always emitting a `while` loop.

We cleaned up the loops manually where it was simple to see that that would be correct. In some cases we can even iterate in a smarter way (e.g. in chunks or windows). But some `while` loops remain where the C code really was doing something sneaky.

### Types and Casting

The `c2rust` output contains many explicit type annotations and integer casts. Those are required to guarantee that the behavior of the Rust code matches that of the C code, but often the type can be changed without changing the semantics.

For instance, C will often use `int` for an integer, even if all runtime values would fit into a Rust `u8`. C will also use `int` for most loop variables, where using `usize` in Rust is much more convenient.

Removing the casts and changing the types is a manual process, and it can be quite time-consuming to convince yourself that using a smaller or unsigned data type is in fact correct.

## Make it safe

Now that we've seen some of the basic patterns, let's look at a slightly more complicated example: insertion sort.

```c
void insertion_sort(int const n, int * const p) {
    for (int i = 1; i < n; i++) {
        int const tmp = p[i];
        int j = i;
        while (j > 0 && p[j-1] > tmp) {
                p[j] = p[j-1];
                j--;
        }
        p[j] = tmp;
    }
}
```

This C code uses pointers, and hence the resulting generated Rust code will definitely be unsafe:

```rust
pub unsafe extern "C" fn insertion_sort(n: libc::c_int, p: *mut libc::c_int) {
    let mut i: libc::c_int = 1 as libc::c_int;
    while i < n {
        let tmp: libc::c_int = *p.offset(i as isize);
        let mut j: libc::c_int = i;
        while j > 0 as libc::c_int && *p.offset((j - 1 as libc::c_int) as isize) > tmp {
            *p.offset(j as isize) = *p.offset((j - 1 as libc::c_int) as isize);
            j -= 1;
            j;
        }
        *p.offset(j as isize) = tmp;
        i += 1;
        i;
    }
}
```

We can apply some quick cleanups:

- `extern "C"` is not relevant for us
- the outer `while` loop can easily become a `for` loop over a range
- remove some of the casting noise

But what makes cleaning up this code challenging is the use of `offset` for all pointer arithmetic. The array index syntax in C, `ptr[index]`, is just syntactic sugar for pointer arithmetic `*(ptr + index)`, in which `index` can absolutely be negative. Because `c2rust` can't statically know that the index won't be negative, it has to consider that it could be. Again, convincing yourself that nothing sneaky is going on can be rather time consuming.

Luckily in this particular case, all is well and we can turn the input into a Rust slice so that the convenient indexing syntax can be used:

```rust
pub fn insertion_sort(p: &mut [libc::c_int]) {
    for i in 1..p.len() {
        let tmp = p[i];
        let mut j = i;
        while j > 0 && p[j - 1] > tmp {
            p[j] = p[j - 1];
            j -= 1;
        }
        p[j] = tmp;
    }
}
```

With that change, the `i` and `j` variables get the type `usize`, and the function is now completely safe.

One final requirement is that all the call sites must be able to supply a  mutable slice. Because Rust slices make strong safety assumptions, that can take some additional work. Using [miri](https://github.com/rust-lang/miri) has been extremely helpful to confirm the correctness of our code.

### Complex control flow

The C language has a couple of control flow mechanisms that don't translate to Rust in a straightforward way. Examples are `goto` statements and `switch` cases that fall through from one branch to another.

(The technical term for such constructs is [irreducible control flow](https://en.wikipedia.org/wiki/Control-flow_graph#Reducibility), which (today) is not possible to express in Rust's surface language.)

The tricky thing is: these constructs most often occur in performance-sensitive code. `c2rust` tries to do its best, but today it's just not always possible to generate code with equivalent performance characteristics.

Even in simple cases, the output is rather ugly. Consider this example:

```c
void adjustValue(int *value, int operation) {
    switch (operation) {
        case 1:  // Increment by 10
            *value += 10;
            // implicitly falls through to the next case
        case 2:  // Multiply by 2
            *value *= 2;
            // implicitly falls through to the next case
        default:  // No operation
            break;
    }
}
```

Here `c2rust` generates this rather messy piece of code:

```rust
pub unsafe extern "C" fn adjustValue(
    mut value: *mut libc::c_int,
    mut operation: libc::c_int,
) {
    let mut current_block_1: u64;
    match operation {
        1 => {
            *value += 10 as libc::c_int;
            current_block_1 = 4407541767199398248;
        }
        2 => {
            current_block_1 = 4407541767199398248;
        }
        _ => {
            current_block_1 = 12675440807659640239;
        }
    }
    match current_block_1 {
        4407541767199398248 => {
            *value *= 2 as libc::c_int;
        }
        _ => {}
    };
}
```

We can try to find some better names for the labels, but otherwise it's not exactly clear how to improve the code without duplicating blocks.

### libc functions

We translated not just the bzip2 library, but also the binary. Here we find things like this:

```c

// Opens a file, but refuses to overwrite an existing one.
static FILE* fopen_output_safely ( Char* name, const char* mode ) {
#  if BZ_UNIX
   FILE*     fp;
   int       fh;
   fh = open(name, O_WRONLY|O_CREAT|O_EXCL, S_IWUSR|S_IRUSR);
   if (fh == -1) return NULL;
   fp = fdopen(fh, mode);
   if (fp == NULL) close(fh);
   return fp;
#  else
   return fopen(name, mode);
#  endif
}
```

Which gets translated as:

```rust
pub unsafe extern "C" fn fopen_output_safely(
    mut name: *mut libc::c_char,
    mut mode: *const libc::c_char,
) -> *mut FILE {
    let mut fp: *mut FILE = 0 as *mut FILE;
    let mut fh: libc::c_int = 0;
    fh = open(
        name,
        0o1 as libc::c_int | 0o100 as libc::c_int | 0o200 as libc::c_int,
        0o200 as libc::c_int | 0o400 as libc::c_int,
    );
    if fh == -(1 as libc::c_int) {
        return 0 as *mut FILE;
    }
    fp = fdopen(fh, mode);
    if fp.is_null() {
        close(fh);
    }
    return fp;
```

This is a mess for several reasons:

- conditional compilation is lost (we only get the unix version here)
- constant names are lost (there is a flag to potentially fix that though)
- using libc directly like this is not ergonomic in Rust

But to clean up this code, you actually need to have a deep understanding of what both the C and Rust calls do. E.g. when you open a file in Rust, how exactly is that different from a C call to `open`?

We eventually turned the function into roughly this:

```rust
use std::path::Path;

#[cfg(unix)]
fn open_output_safely(name: &Path) -> Option<*mut libc::FILE> {
    use std::os::fd::IntoRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    use std::ffi::c_char;

    const WB_MODE: *const c_char = b"wb\0".as_ptr().cast::<c_char>();

    let mut opts = std::fs::File::options();

    opts.write(true).create_new(true);

    #[allow(clippy::unnecessary_cast)]
    opts.mode((libc::S_IWUSR | libc::S_IRUSR) as u32);

    let file = opts.open(name).ok()?;

    let fd = file.into_raw_fd();
    let fp = unsafe { libc::fdopen(fd, WB_MODE) };
    if fp.is_null() {
        unsafe { libc::close(fd) };
        return None;
    }

    Some(fp)
}
```

The `open` call did get replaced by a Rust standard library call, but `fdopen` is still needed because other code relies on using `FILE` handles (e.g. so that `stdout` and `stdin` can be handled, which would not work well with Rust's `std::file::File`).

Cleaning up this kind of code is just not that fun. So while it was interesting as an experiment, we'd recommend mostly just starting from scratch for such stdio-heavy pieces of code: the end result will be much nicer and more coherent.

## Testing

One of the major advantages of `c2rust` is that on day one you get a bug-for-bug compatible translation of the C code: the behavior is identical. Whereas when you rewrite manually or start from scratch you get to introduce your own bugs.

It is therefore important to maintain that compatibility, and not introduce any new bugs while cleaning up the code. Most of the cleanup is straightforward, but there are tricky cases.

For example, we noticed that (old) C code is often not very precise about the integer types that they use, both with regards to size and signedness. But changing those characteristics requires a careful look at the source code to make sure no overflows occur.

So, getting the original test suite to run with your implementation somehow is a good idea. We ported the test cases (using the original test data, but `cargo test` to actually run the tests) later than we should have.

Working on those tests when there is so much low-hanging refactoring fruit ready to be picked is unattractive (bash, python, arcane C: not fun), but extremely valuable.

### Fuzzing

Having two implementations that should give the same output given the same input is an ideal case for differential fuzzing: we feed the Rust and C implementation the same input bytes, and assert that they produce the same output bytes (or the same error for invalid input).

However, we must be careful with the fuzzing giving a false sense of security: we only fuzz with a relatively recent version of bzip2, but still need to be able to decode files that were compressed with much older (like, 10+ years) versions of bzip2, that use features of the file format that a modern compressor doesn't use.

### Benchmarking

We set up a dashboard to track performance, which can be found [here](https://trifectatechfoundation.github.io/libbzip2-rs-bench/). Overall we've improved on compression speed, and see some improvements and some regressions for decompression speed.

A caveat with these benchmarks is that they are somewhat unreliable. Compared to e.g. zlib, bzip2 uses a lot more RAM, and is a lot slower, so there is more noise and we can't get as many samples within a given amount of time.

## Conclusion

For the library portion, we are extremely happy with the process and result of using `c2rust`. On day one, we had a library that just worked like the original, and we are fairly confindent that the implementation is correct (in the sense that it is bug-for-bug compatible with the C version). Performance is also good (more on that in the future).

For the binary portion, we believe the effort spent on cleanup was not worth it. We'd recommend that either you use `c2rust` for the translation and then leave it (it's no more unsafe than the C code you'd otherwise use), or start from scratch in Rust.

While much of this post focusses on cases where `c2rust` does not provide idiomatic translations, we do believe it is much better than the alternative of manually translating code. Furthermore `c2rust` continues to improve and hopefully will be able to produce better output for some of the cases we've seen.

Overall we're very happy with this experiment, and we do intend to use `c2rust` again for other components of our [data compression initiative](https://trifectatech.org/initiatives/data-compression/), starting with some of the remaining portions of zlib-rs.

You can look at the  [libbzip2-rs](https://github.com/trifectatechfoundation/libbzip2-rs) source code on github, and use it today if your project uses the [bzip2](https://crates.io/crates/bzip2) crate, by enabling the `libbz2-rs-sys` feature gate.
