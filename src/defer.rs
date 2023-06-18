pub(crate) struct ScopeCall<F: FnMut()> {
    pub(crate) c: F,
}

impl<F: FnMut()> Drop for ScopeCall<F> {
    fn drop(&mut self) {
        (self.c)();
    }
}

/// Mimics defer() in Go via the use of Drop descructors.
/// Concept lifted off https://stackoverflow.com/questions/29963449/golang-like-defer-in-rust
#[macro_export]
macro_rules! defer {
    ($($data: tt)*) => (
        let _scope_call = $crate::defer::ScopeCall {
            c: || -> () { $($data)* }
        };
    )
}
