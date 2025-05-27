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
