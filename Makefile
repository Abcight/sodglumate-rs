setup:
	sudo apt-get update
	sudo apt-get install -y clang libavcodec-dev libavformat-dev libavutil-dev libavfilter-dev libavdevice-dev libswscale-dev libswresample-dev

run:
	cargo run --release