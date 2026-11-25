# Undavi documentation

These pages explain how Undavi works and how to work on it. For installing and
using the app, see the [README](../README.md); for setting up a development
environment, see [CONTRIBUTING](../CONTRIBUTING.md).

| Page | What it covers |
|---|---|
| [Architecture](architecture.md) | The modules, the threads, and how a frame is made. |
| [Audio pipeline](audio-pipeline.md) | From captured samples to bar heights, step by step, with every constant. |
| [Settings](settings.md) | Where settings live, the file format, versioning and migrations. |
| [Shaders](shaders.md) | Writing your own visualizers and backgrounds: the three kinds, declaring settings, the helpers, errors. |
| [Packaging](packaging.md) | Installing, the Arch package, desktop integration, and what other formats need. |
| [Releasing](releasing.md) | Putting out a new version: checklist, tagging, the release workflow, the AUR. |
| [Writing docs](writing-docs.md) | How code comments, API docs and these pages are written. |

## API reference

The library's API docs are generated from the code:

```sh
cargo doc --no-deps --open
```

They cover the public modules (`audio`, `spectrum`, `settings`, `lyrics` and
`media`); every public item is documented, and the build warns when one isn't.
