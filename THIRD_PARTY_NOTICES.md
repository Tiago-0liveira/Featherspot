# Third-party notices

lspotify's own source code is licensed under the repository's [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE) license. Dependencies remain under their respective licenses.

The exact dependency versions shipped by a build are recorded in `Cargo.lock`; release SBOMs and
the repository's `deny.toml` are used to keep dependency/license information auditable.

## Librespot

Project: https://github.com/librespot-org/librespot  
License: MIT  
Copyright (c) 2015 Paul Lietar

lspotify embeds Librespot directly for lightweight local playback. Librespot is an independent
project and is not affiliated with or endorsed by Spotify.

The Librespot MIT license notice is reproduced below as required by that license:

> The MIT License (MIT)
>
> Copyright (c) 2015 Paul Lietar
>
> Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
> associated documentation files (the "Software"), to deal in the Software without restriction,
> including without limitation the rights to use, copy, modify, merge, publish, distribute,
> sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all copies or
> substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT
> NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
> NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
> DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT
> OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## Other dependencies

lspotify also depends on third-party Rust crates such as Ratatui, Crossterm, Wry, Rodio/CPAL,
Reqwest, Rusqlite, Tokio, Serde, and their transitive dependencies. `Cargo.lock` is the
authoritative version inventory for a release.
