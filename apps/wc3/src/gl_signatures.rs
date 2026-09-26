//! Signatures for the OPENGL32 imports extracted from Game.dll.
//! The thunk supplies cleanup, while this table supplies argument meaning and
//! the policy for a temporarily unimplemented call.

use crate::process::{GuestMemory, read_guest_words};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GlEffect {
    /// A void state setter may be noted on the current HGLRC during bringup.
    NoteWrite,
    /// Results, resource mutation, draws, and context lifetime need semantics.
    Frontier,
}

#[derive(Clone, Copy, Debug)]
pub enum GlArgKind {
    Enum,
    Int,
    Uint,
    Boolean,
    Float,
    DoubleLow,
    DoubleHigh,
    Pointer,
    FloatArray(usize),
    PnameFloats,
    CString,
}

#[derive(Clone, Copy, Debug)]
pub struct GlArg {
    pub name: &'static str,
    pub kind: GlArgKind,
}

#[derive(Clone, Copy, Debug)]
pub struct GlSignature {
    pub symbol: &'static str,
    pub effect: GlEffect,
    pub args: &'static [GlArg],
}

macro_rules! sig {
    ($symbol:literal, $effect:ident, [$($name:literal : $kind:expr),* $(,)?]) => {
        GlSignature {
            symbol: $symbol,
            effect: GlEffect::$effect,
            args: &[$(GlArg { name: $name, kind: $kind }),*],
        }
    };
}

use GlArgKind::{Boolean as B, CString as S, Enum as E, Float as F, Int as I, Pointer as P, Uint as U};

pub const GL_SIGNATURES: &[GlSignature] = &[
    sig!("wglMakeCurrent", Frontier, ["hdc": P, "hglrc": P]),
    sig!("glDisable", Frontier, ["cap": E]),
    sig!("glEnable", Frontier, ["cap": E]),
    sig!("glLightfv", Frontier, ["light": E, "pname": E, "params": GlArgKind::PnameFloats]),
    sig!("glFogfv", Frontier, ["pname": E, "params": GlArgKind::PnameFloats]),
    sig!("glFogf", Frontier, ["pname": E, "param": F]),
    sig!("glFogi", Frontier, ["pname": E, "param": I]),
    sig!("glDrawBuffer", Frontier, ["mode": E]),
    sig!("glDepthFunc", Frontier, ["func": E]),
    sig!("glAlphaFunc", Frontier, ["func": E, "ref": F]),
    sig!("glBlendFunc", Frontier, ["source": E, "destination": E]),
    sig!("glEnableClientState", Frontier, ["array": E]),
    sig!("glTexEnvi", Frontier, ["target": E, "pname": E, "param": I]),
    sig!("glBindTexture", Frontier, ["target": E, "texture": U]),
    sig!("glDisableClientState", Frontier, ["array": E]),
    sig!("glDepthMask", Frontier, ["flag": B]),
    sig!("glColorMaterial", Frontier, ["face": E, "mode": E]),
    sig!("glTexGeni", Frontier, ["coord": E, "pname": E, "param": I]),
    sig!("glLightModelfv", Frontier, ["pname": E, "params": GlArgKind::PnameFloats]),
    sig!("glMaterialfv", Frontier, ["face": E, "pname": E, "params": GlArgKind::PnameFloats]),
    sig!("glPolygonOffset", Frontier, ["factor": F, "units": F]),
    sig!("glGetIntegerv", Frontier, ["pname": E, "output": P]),
    sig!("wglGetProcAddress", Frontier, ["name": S]),
    sig!("glGetString", Frontier, ["name": E]),
    sig!("wglCreateContext", Frontier, ["hdc": P]),
    sig!("wglDeleteContext", Frontier, ["hglrc": P]),
    sig!("glDeleteTextures", Frontier, ["count": I, "textures": P]),
    sig!("glTexSubImage2D", Frontier, ["target": E, "level": I, "xoffset": I, "yoffset": I, "width": I, "height": I, "format": E, "kind": E, "pixels": P]),
    sig!("glTexImage2D", Frontier, ["target": E, "level": I, "internal_format": I, "width": I, "height": I, "border": I, "format": E, "kind": E, "pixels": P]),
    sig!("glPixelStorei", Frontier, ["pname": E, "param": I]),
    sig!("glTexParameteri", Frontier, ["target": E, "pname": E, "param": I]),
    sig!("glGenTextures", Frontier, ["count": I, "output": P]),
    sig!("glNormal3fv", Frontier, ["normal": GlArgKind::FloatArray(3)]),
    sig!("glNormalPointer", Frontier, ["kind": E, "stride": I, "pointer": P]),
    sig!("glVertexPointer", Frontier, ["size": I, "kind": E, "stride": I, "pointer": P]),
    sig!("glColorPointer", Frontier, ["size": I, "kind": E, "stride": I, "pointer": P]),
    sig!("glTexCoordPointer", Frontier, ["size": I, "kind": E, "stride": I, "pointer": P]),
    sig!("glFinish", Frontier, []),
    sig!("glDrawElements", Frontier, ["mode": E, "count": I, "kind": E, "indices": P]),
    sig!("glLoadMatrixf", Frontier, ["matrix": GlArgKind::FloatArray(16)]),
    sig!("glMatrixMode", Frontier, ["mode": E]),
    sig!("glScissor", Frontier, ["x": I, "y": I, "width": I, "height": I]),
    sig!("glDepthRange", Frontier, ["near": GlArgKind::DoubleLow, "_near_hi": GlArgKind::DoubleHigh, "far": GlArgKind::DoubleLow, "_far_hi": GlArgKind::DoubleHigh]),
    sig!("glViewport", Frontier, ["x": I, "y": I, "width": I, "height": I]),
    sig!("glClear", Frontier, ["mask": U]),
    sig!("glClearColor", Frontier, ["red": F, "green": F, "blue": F, "alpha": F]),
    sig!("glReadPixels", Frontier, ["x": I, "y": I, "width": I, "height": I, "format": E, "kind": E, "output": P]),
    sig!("glReadBuffer", Frontier, ["mode": E]),
    sig!("wglSwapLayerBuffers", Frontier, ["hdc": P, "planes": U]),
    sig!("glLightf", Frontier, ["light": E, "pname": E, "param": F]),
];

pub fn signature(symbol: &str) -> Option<&'static GlSignature> {
    GL_SIGNATURES.iter().find(|entry| entry.symbol == symbol)
}

pub struct GlCall {
    pub words: Vec<u32>,
    pub description: String,
}

pub fn decode_call(
    signature: &GlSignature,
    memory: &impl GuestMemory,
    esp: u32,
) -> Result<GlCall, String> {
    use std::fmt::Write;

    let frame = read_guest_words(memory, esp, signature.args.len() + 1)?;
    let words = frame[1..].to_vec();
    let mut description = String::new();
    for (index, arg) in signature.args.iter().enumerate() {
        if matches!(arg.kind, GlArgKind::DoubleHigh) {
            continue;
        }
        if !description.is_empty() {
            description.push(' ');
        }
        let word = words[index];
        write!(&mut description, "{}=", arg.name).unwrap();
        match arg.kind {
            GlArgKind::Enum | GlArgKind::Uint => write!(&mut description, "0x{word:08x}").unwrap(),
            GlArgKind::Int => write!(&mut description, "{}", word as i32).unwrap(),
            GlArgKind::Boolean => write!(&mut description, "{}", word != 0).unwrap(),
            GlArgKind::Float => write!(&mut description, "{}", f32::from_bits(word)).unwrap(),
            GlArgKind::DoubleLow => {
                let bits = u64::from(word) | (u64::from(words[index + 1]) << 32);
                write!(&mut description, "{}", f64::from_bits(bits)).unwrap();
            }
            GlArgKind::DoubleHigh => unreachable!(),
            GlArgKind::Pointer => write!(&mut description, "0x{word:08x}").unwrap(),
            GlArgKind::CString => {
                let value = if word == 0 {
                    "<null>".to_owned()
                } else {
                    crate::process::read_c_string(memory, word, 128).map_err(str::to_owned)?
                };
                write!(&mut description, "0x{word:08x}:{value:?}").unwrap();
            }
            GlArgKind::FloatArray(count) => append_floats(&mut description, memory, word, count)?,
            GlArgKind::PnameFloats => {
                let pname = words[index - 1];
                let count = match pname {
                    0x0b66 | 0x0b53 | 0x1200..=0x1203 | 0x1600 | 0x1602 => 4,
                    0x1204 => 3,
                    _ => 1,
                };
                append_floats(&mut description, memory, word, count)?;
            }
        }
    }
    Ok(GlCall { words, description })
}

fn append_floats(
    description: &mut String,
    memory: &impl GuestMemory,
    address: u32,
    count: usize,
) -> Result<(), String> {
    use std::fmt::Write;

    if address == 0 {
        description.push_str("<null>");
        return Ok(());
    }
    let mut raw = vec![0; count * 4];
    memory.read(address, &mut raw).map_err(str::to_owned)?;
    write!(description, "0x{address:08x}:[").unwrap();
    for (index, word) in raw.chunks_exact(4).enumerate() {
        if index != 0 {
            description.push_str(", ");
        }
        write!(description, "{}", f32::from_le_bytes(word.try_into().unwrap())).unwrap();
    }
    description.push(']');
    Ok(())
}
