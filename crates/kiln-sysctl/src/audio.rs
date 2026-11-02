//! Audio endpoint switching via IPolicyConfig or stubs.
use kiln_core::error::SysctlError;
use kiln_core::traits::AudioApi;

pub struct WindowsAudio;

impl WindowsAudio {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsAudio {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioApi for WindowsAudio {
    fn get_default_endpoint(&self) -> Result<String, SysctlError> {
        #[cfg(windows)]
        {
            // Windows implementation placeholder
            Ok("DefaultAudioDevice".to_string())
        }
        #[cfg(not(windows))]
        {
            Ok("LinuxAudioStub".to_string())
        }
    }

    fn set_default_endpoint(&self, _endpoint_id: &str) -> Result<(), SysctlError> {
        #[cfg(windows)]
        {
            // Windows implementation placeholder
            Ok(())
        }
        #[cfg(not(windows))]
        {
            Ok(())
        }
    }
}
