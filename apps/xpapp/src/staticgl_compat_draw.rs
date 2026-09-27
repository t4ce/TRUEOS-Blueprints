// WGL lifecycle and readback boundary; draws use staticgl_gpu_draw.rs.
#[cfg(test)]
include!("staticgl_cpu_reference.rs");

fn gl_gpu_backend_required(api: &'static str) -> ProviderDispatchError {
    gl_texture_error(api, "native GPU readback not connected; CPU framebuffer removed")
}

impl XpProcess {
    fn wgl_get_proc_address_static(
        &self,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, name] = arguments::<2>(memory, esp)?;
        let _name = read_c_string(memory, name, 256)?;
        // No extensions are advertised. Static GL1.1 imports use their existing thunks.
        Ok(0)
    }
    fn wgl_delete_context_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, handle] = arguments::<2>(memory, esp)?;
        let Some(runtime) = self.gl_runtime.as_mut() else {
            return Ok(0);
        };
        let Some(c) = runtime.contexts.get(&handle) else {
            return Ok(0);
        };
        if c.current_tid.is_some_and(|owner| owner != tid) {
            return Ok(0);
        }
        runtime.contexts.remove(&handle);
        Ok(1)
    }
    fn gl_read_buffer_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, mode] = arguments::<2>(memory, esp)?;
        self.gl_context_mut(tid, "glReadBuffer")?;
        if mode != 0x405 {
            return Err(gl_texture_error(
                "glReadBuffer",
                format!("unsupported read buffer0x{mode:x}"),
            ));
        }
        Ok(0)
    }
    fn gl_read_pixels_static(
        &mut self,
        tid: u32,
        _esp: u32,
        _memory: &mut impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        self.gl_context_mut(tid, "glReadPixels")?;
        Err(gl_gpu_backend_required("glReadPixels"))
    }
    pub fn gl_preview_pending(&self, tid: u32) -> bool {
        cfg!(feature = "preview-first-draw")
            && self
                .gl_runtime
                .as_ref()
                .and_then(|r| r.contexts.values().find(|c| c.current_tid == Some(tid)))
                .is_some_and(|c| c.draw_count == 0)
    }
    fn gl_present_gpu(
        &mut self,
        tid: u32,
        _reason: &str,
    ) -> Result<(), ProviderDispatchError> {
        // Every native submission retires before its upload buffers are reused.
        // The coordinator publishes this completed UI4 frame at the WGL boundary.
        self.gl_context_mut(tid, "OpenGL present")?.swap_count += 1;
        Ok(())
    }
}

#[cfg(test)]
mod staticgl_compat_tests {
    use super::*;
    include!("staticgl_compat_tests.rs");
}
