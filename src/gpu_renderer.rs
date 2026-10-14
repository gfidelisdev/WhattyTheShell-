use crate::config::{ConfigManager, GpuBackend};

#[derive(Debug)]
pub struct GPURenderer {
    pub backend: GpuBackend,
}

impl GPURenderer {
    /// Choose the best available backend, respecting any user‑override.
    pub fn new(cfg: &ConfigManager) -> Self {
        // 1️⃣ Respect explicit user choice.
        if let Some(pref) = cfg.gpu_backend_preference() {
            return GPURenderer { backend: pref };
        }
        // 2️⃣ Feature‑based detection – prefer Vulkan, then OpenGL, then Software.
        #[cfg(feature = "vulkan")]
        let backend = GpuBackend::Vulkan;
        #[cfg(all(not(feature = "vulkan"), feature = "opengl"))]
        let backend = GpuBackend::OpenGl;
        #[cfg(not(any(feature = "vulkan", feature = "opengl")))]
        let backend = GpuBackend::Software;
        GPURenderer { backend }
    }

    #[allow(dead_code)]
    pub fn backend_name(&self) -> &'static str {
        match self.backend {
            GpuBackend::Vulkan => "Vulkan",
            GpuBackend::OpenGl => "OpenGL",
            GpuBackend::Software => "Software (CPU)",
        }
    }
}
