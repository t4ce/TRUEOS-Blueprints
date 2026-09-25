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
    sig!("glDisable", NoteWrite, ["cap": E]),
    sig!("glEnable", NoteWrite, ["cap": E]),
    sig!("glLightfv", NoteWrite, ["light": E, "pname": E, "params": GlArgKind::PnameFloats]),
    sig!("glFogfv", NoteWrite, ["pname": E, "params": GlArgKind::PnameFloats]),
    sig!("glFogf", NoteWrite, ["pname": E, "param": F]),
    sig!("glFogi", NoteWrite, ["pname": E, "param": I]),
    sig!("glDrawBuffer", NoteWrite, ["mode": E]),
    sig!("glDepthFunc", NoteWrite, ["func": E]),
    sig!("glAlphaFunc", NoteWrite, ["func": E, "ref": F]),
    sig!("glBlendFunc", NoteWrite, ["source": E, "destination": E]),
    sig!("glEnableClientState", NoteWrite, ["array": E]),
    sig!("glTexEnvi", NoteWrite, ["target": E, "pname": E, "param": I]),
    sig!("glBindTexture", NoteWrite, ["target": E, "texture": U]),
    sig!("glDisableClientState", NoteWrite, ["array": E]),
    sig!("glDepthMask", NoteWrite, ["flag": B]),
    sig!("glColorMaterial", NoteWrite, ["face": E, "mode": E]),
    sig!("glTexGeni", NoteWrite, ["coord": E, "pname": E, "param": I]),
    sig!("glLightModelfv", NoteWrite, ["pname": E, "params": GlArgKind::PnameFloats]),
    sig!("glMaterialfv", NoteWrite, ["face": E, "pname": E, "params": GlArgKind::PnameFloats]),
    sig!("glPolygonOffset", NoteWrite, ["factor": F, "units": F]),
    sig!("glGetIntegerv", Frontier, ["pname": E, "output": P]),
    sig!("wglGetProcAddress", Frontier, ["name": S]),
    sig!("glGetString", Frontier, ["name": E]),
    sig!("wglCreateContext", Frontier, ["hdc": P]),
    sig!("wglDeleteContext", Frontier, ["hglrc": P]),
    sig!("glDeleteTextures", Frontier, ["count": I, "textures": P]),
    sig!("glTexSubImage2D", Frontier, ["target": E, "level": I, "xoffset": I, "yoffset": I, "width": I, "height": I, "format": E, "kind": E, "pixels": P]),
    sig!("glTexImage2D", Frontier, ["target": E, "level": I, "internal_format": I, "width": I, "height": I, "border": I, "format": E, "kind": E, "pixels": P]),
    sig!("glPixelStorei", NoteWrite, ["pname": E, "param": I]),
    sig!("glTexParameteri", NoteWrite, ["target": E, "pname": E, "param": I]),
    sig!("glGenTextures", Frontier, ["count": I, "output": P]),
    sig!("glNormal3fv", NoteWrite, ["normal": GlArgKind::FloatArray(3)]),
    sig!("glNormalPointer", NoteWrite, ["kind": E, "stride": I, "pointer": P]),
    sig!("glVertexPointer", NoteWrite, ["size": I, "kind": E, "stride": I, "pointer": P]),
    sig!("glColorPointer", NoteWrite, ["size": I, "kind": E, "stride": I, "pointer": P]),
    sig!("glTexCoordPointer", NoteWrite, ["size": I, "kind": E, "stride": I, "pointer": P]),
    sig!("glFinish", Frontier, []),
    sig!("glDrawElements", Frontier, ["mode": E, "count": I, "kind": E, "indices": P]),
    sig!("glLoadMatrixf", NoteWrite, ["matrix": GlArgKind::FloatArray(16)]),
    sig!("glMatrixMode", NoteWrite, ["mode": E]),
    sig!("glScissor", NoteWrite, ["x": I, "y": I, "width": I, "height": I]),
    sig!("glDepthRange", NoteWrite, ["near": GlArgKind::DoubleLow, "_near_hi": GlArgKind::DoubleHigh, "far": GlArgKind::DoubleLow, "_far_hi": GlArgKind::DoubleHigh]),
    sig!("glViewport", NoteWrite, ["x": I, "y": I, "width": I, "height": I]),
    sig!("glClear", Frontier, ["mask": U]),
    sig!("glClearColor", NoteWrite, ["red": F, "green": F, "blue": F, "alpha": F]),
    sig!("glReadPixels", Frontier, ["x": I, "y": I, "width": I, "height": I, "format": E, "kind": E, "output": P]),
    sig!("glReadBuffer", NoteWrite, ["mode": E]),
    sig!("wglSwapLayerBuffers", Frontier, ["hdc": P, "planes": U]),
    sig!("glLightf", NoteWrite, ["light": E, "pname": E, "param": F]),
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
