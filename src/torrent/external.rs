use std::error::Error;
use std::process::Command;

pub fn open_magnet_in_default_client(magnet: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    #[cfg(windows)]
    {
    
        Command::new("cmd")
            .args(["/C", "start", "", magnet])
            .spawn()?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(magnet).spawn()?;
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open").arg(magnet).spawn()?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err("Unsupported platform for opening magnet links".into())
}