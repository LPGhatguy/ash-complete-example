// A handy little macro that lets us specify C-style strings
macro_rules! cstr {
    ($s:expr) => (
        concat!($s, "\0") as *const str as *const [i8] as *const i8
    );
}