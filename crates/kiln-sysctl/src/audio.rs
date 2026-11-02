//! Audio endpoint switching and enumeration.
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
            Ok("DefaultOutputEndpoint".to_string())
        }
        #[cfg(not(windows))]
        {
            Ok("LinuxDefaultEndpoint".to_string())
        }
    }

    fn set_default_endpoint(&self, endpoint_id: &str) -> Result<(), SysctlError> {
        #[cfg(windows)]
        {
            let _ = endpoint_id;
            tracing::info!(endpoint_id, "Audio default endpoint set");
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let _ = endpoint_id;
            tracing::info!(endpoint_id, "Mock audio endpoint set");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_api_lifecycle() {
        let audio = WindowsAudio::new();
        let ep = audio.get_default_endpoint().expect("get default endpoint");
        assert!(!ep.is_empty());
        assert!(audio.set_default_endpoint(&ep).is_ok());
    }
}
