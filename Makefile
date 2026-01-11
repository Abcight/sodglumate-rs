setup:
	sudo apt-get update
	sudo apt-get install -y clang libavcodec-dev libavformat-dev libavutil-dev libavfilter-dev libavdevice-dev libswscale-dev libswresample-dev
	sudo apt-get install -y gcc-multilib mingw-w64
	rustup target add i686-unknown-linux-gnu i686-pc-windows-gnu

run:
	cargo run --release

build-win32:
	cargo build --release --target i686-pc-windows-gnu