class Kget < Formula
  desc "Fast, modern download manager and Rust library — HTTP/HTTPS, FTP/SFTP, WebDAV, torrent, yt-dlp"
  homepage "https://github.com/davimf721/KGet"
  url "https://github.com/davimf721/KGet/archive/refs/tags/v1.7.0.tar.gz"
  sha256 "98f5c79b2ee640babf9fd6320b08243a1a4ca8e7fa2ea0df181963ccc6857f3f" # updated automatically by release.sh
  license "MIT"
  head "https://github.com/davimf721/KGet.git", branch: "main"

  depends_on "rust" => :build

  # GUI (egui) requires native display libraries on Linux
  on_linux do
    depends_on "libxcb" if build.with?("gui")
    depends_on "pkg-config" => :build
    depends_on "openssl@3"
  end

  option "with-gui",     "Build with egui graphical interface (Linux/macOS/Windows)"
  option "with-torrent", "Build with native BitTorrent/magnet client (librqbit)"

  def install
    features = []
    features << "gui"            if build.with?("gui")
    features << "torrent-native" if build.with?("torrent")

    cargo_args = std_cargo_args
    unless features.empty?
      cargo_args += ["--features", features.join(",")]
    end

    system "cargo", "install", *cargo_args
  end

  def caveats
    caveats_text = <<~EOS
      KGet #{version} installed.

      Quick start:
        kget https://example.com/file.zip          # basic download
        kget -a https://example.com/large.iso      # turbo (parallel connections)
        kget --interactive                          # interactive REPL

    EOS
    if build.with?("gui")
      caveats_text += "  Launch the GUI:\n    kget --gui\n\n"
    end
    if build.with?("torrent")
      caveats_text += "  BitTorrent / magnet links are enabled.\n\n"
    end
    caveats_text
  end

  test do
    assert_match "kget #{version}", shell_output("#{bin}/kget --version")
  end
end
