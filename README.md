# Sodglumate
A native media browser for [e621](https://e621.net).

<kbd><img src="README.png"></img></kbd>

## About

> [!WARNING]  
> This application is intended for adult use only.

Sodglumate is a desktop application for browsing and viewing media from e621.

Designed with mostly single-handed operation in mind.

### Features
- Tag-based search with pagination
- Support for images (JPEG, PNG, WebP, GIF) and videos (MP4, WebM)
- Automatic slideshow with configurable timing
- Auto-panning for images larger than viewport
- Aggressive prefetching for seamless browsing
- Built-in "breathing timer" 😉\*

\* *The breathing timer is intended to be used at your own risk. Sodglumate is not a medical app.*

### Links

This project makes use of e621's API. For more information, see the [e621.net API Documentation](https://e621.net/wiki_pages/help:api).

## Downloads

Pre-built binaries are available for download on the [Releases](https://github.com/abcight/sodglumate-rs/releases) page.

The newest binaries are as follows:
* [Windows x86_64]()
* [Linux x86_64]()

## Issues & Feedback

Please report any issues or feedback on the [Issues](https://github.com/abcight/sodglumate-rs/issues) page.

Alternatively, you can also contact me on Discord at `abcight`. Note that I am not always online, and may not accept your friend request.

## Building from Source

### Pre-Requisites

* [Rust](https://www.rust-lang.org/tools/install) (2024 edition)

On Ubuntu/Debian, install system dependencies with:

```sh
make setup
```

### Building

```sh
make build
```

The result will be a `sodglumate-rs` binary found in `target/release`.

### Running

```sh
make run
```

### Search Syntax

The search bar accepts e621's tag syntax. Examples:

```
wolf solo order:score
~male ~female rating:safe
artist:husdingo -comic
```

See [e621 cheatsheet](https://e621.net/wiki_pages/help:cheatsheet) for full syntax.

## Contributing

See [CONTRIBUTING](CONTRIBUTING) for guidelines. All contributions require
sign-off per the [Developer Certificate of Origin](DCO).

## License

This project is licensed under the GNU Affero General Public License v3.0.
See [LICENSE](LICENSE) for the full license text.
