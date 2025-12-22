/// Print a message to the console if not in quiet mode.
/// 
/// # Arguments
/// 
/// * `msg` - The message to print
/// * `quiet_mode` - If true, suppress printing the message
pub fn print(msg: &str, quiet_mode: bool) {
    if !quiet_mode {
        println!("{}", msg);
    }
}

/// Tries to extract the filename from a URL.
/// If the URL cannot be parsed or does not contain a filename in the path,
/// it returns the provided default filename.
///
/// # Arguments
///
/// * `url_str` - A string slice of the URL to extract the filename from.
/// * `default_filename` - The filename to return if none can be extracted from the URL.
///
/// # Returns
///
/// A `String` containing the extracted filename or the default filename.
pub fn get_filename_from_url_or_default(url_str: &str, default_filename: &str) -> String {
    // Tries to parse the URL
    if let Ok(parsed_url) = url::Url::parse(url_str) {
        // Tries to get the last segment of the path
        if let Some(segments) = parsed_url.path_segments() {
            if let Some(last_segment) = segments.last() {
                if !last_segment.is_empty() {
                    return last_segment.to_string();
                }
            }
        }
    }
    // Returns the default filename if parsing fails or the path is empty/invalid
    default_filename.to_string()
}

/// Resolves the final output path.
/// If output_arg is None, extracts filename from URL.
/// If output_arg is a directory, appends filename from URL.
/// If output_arg is a file path, uses it as is.
pub fn resolve_output_path(output_arg: Option<String>, url: &str, default_name: &str) -> String {
    if let Some(path_str) = output_arg {
        let path = std::path::Path::new(&path_str);
        if path.is_dir() {
            let filename = get_filename_from_url_or_default(url, default_name);
            path.join(filename).to_string_lossy().to_string()
        } else {
            path_str
        }
    } else {
        get_filename_from_url_or_default(url, default_name)
    }
}

