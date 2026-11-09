//! Runs a frame's script without keeping its text.
//!
//! QuickJS keeps a copy of each function's source for `toString`, and in a minified bundle the copy of each function
//! holds all the functions inside it again. For React and its reconciler that is about a quarter of the heap the
//! bundle takes (measured: 702 KB as is, 520 KB without), and kept once for each frame. So a script is compiled,
//! written out as bytecode without its source, and read back from those bytes, which this engine wrote a moment ago.
//! Line numbers stay, so an error says where it came from as before; `toString` of a function says `[native code]`.

use rquickjs::{Ctx, Error, Value, qjs};
use std::ffi::CString;

/// Runs `source` as a classic script in `ctx`, in strict mode as `ctx.eval` does, and returns what it evaluates to. A syntax error,
/// or what the script throws, is `Error::Exception`, which `CatchResultExt::catch` turns into the error.
pub(super) fn run<'js>(ctx: &Ctx<'js>, source: &str) -> rquickjs::Result<Value<'js>> {
    let text = CString::new(source)?;
    let raw = ctx.as_raw().as_ptr();
    // SAFETY: `raw` is the live context of `ctx`. Each value made here is freed once, or handed to `Value`, which frees
    // it; `JS_EvalFunction` frees the function it is given. The bytes read back are bytecode this engine just wrote.
    unsafe {
        let compiled = qjs::JS_Eval(
            raw,
            text.as_ptr(),
            source.len() as _,
            c"eval_script".as_ptr(),
            // Strict, as `ctx.eval` runs a script.
            (qjs::JS_EVAL_TYPE_GLOBAL | qjs::JS_EVAL_FLAG_STRICT | qjs::JS_EVAL_FLAG_COMPILE_ONLY) as i32,
        );
        if qjs::JS_IsException(compiled) {
            return Err(Error::Exception);
        }
        let mut len: qjs::size_t = 0;
        let bytes = qjs::JS_WriteObject(
            raw,
            &mut len,
            compiled,
            (qjs::JS_WRITE_OBJ_BYTECODE | qjs::JS_WRITE_OBJ_STRIP_SOURCE) as i32,
        );
        qjs::JS_FreeValue(raw, compiled);
        if bytes.is_null() {
            return Err(Error::Exception);
        }
        let function = qjs::JS_ReadObject(raw, bytes, len, qjs::JS_READ_OBJ_BYTECODE as i32);
        qjs::js_free(raw, bytes.cast());
        if qjs::JS_IsException(function) {
            return Err(Error::Exception);
        }
        let value = qjs::JS_EvalFunction(raw, function);
        if qjs::JS_IsException(value) {
            return Err(Error::Exception);
        }
        Ok(Value::from_raw(ctx.clone(), value))
    }
}
