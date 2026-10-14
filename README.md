# traest

A small HTTP service that turns shell commands into named, invokable functions.

You register a command (for example `tr a-z A-Z`) under a name. A later request
to that name pipes the request body into the command's stdin and returns its
stdout as the response body. Think of it as a minimal function-as-a-service
runtime backed by local processes.

Built with [axum](https://github.com/tokio-rs/axum) and
[tokio](https://tokio.rs).

## Requirements

- Rust toolchain (edition 2024, so Rust 1.85 or newer)
- Any commands you register must be present on the host's `PATH`

## Running

```sh
cargo run
```

The server listens on `localhost:8080`. There is no configuration; the address
and the 2 second invocation timeout are compiled in (see `src/main.rs`).

## API

| Method | Path              | Description                                         |
| ------ | ----------------- | --------------------------------------------------- |
| GET    | `/health`         | Returns `ok`                                        |
| GET    | `/functions`      | JSON array of registered function names, sorted     |
| POST   | `/functions`      | Register a new function                             |
| POST   | `/invoke/{name}`  | Run a function with the raw request body as stdin   |

### Register a function

```sh
curl -X POST localhost:8080/functions \
  -H 'content-type: application/json' \
  -d '{"name": "upper", "command": "tr", "args": ["a-z", "A-Z"]}'
```

Returns `201 Created`. Registering a name that already exists returns
`409 Conflict`.

### Invoke a function

```sh
curl -X POST localhost:8080/invoke/upper --data-binary 'hello, world'
# HELLO, WORLD
```

The request body is written to the process's stdin as-is. On success the
process's stdout is returned verbatim with `200 OK`.

### List functions

```sh
curl localhost:8080/functions
# ["upper"]
```

### Error responses

| Status | When                                                           |
| ------ | -------------------------------------------------------------- |
| 404    | No function registered under that name                         |
| 409    | A function with that name already exists                       |
| 500    | The command exited non-zero (stderr is included), or an I/O or internal error occurred |
| 504    | The invocation did not complete within 2 seconds               |

Error bodies are plain text describing the failure.

## Library

The crate also exposes a library (`src/lib.rs`) that the binary is a thin
wrapper around:

- `Data` trait: anything that maps input bytes to output bytes
- `Process`: a `Data` implementation that spawns a command and pipes stdin/stdout
- `Unit`: a trivial `Data` implementation (uppercases input) used in tests
- `Registry`: a name-to-`Data` map with `register`, `list`, and `invoke_by_name`
- `InvokeError`: the error type, which also implements axum's `IntoResponse`

## Testing

```sh
cargo test
```

The tests exercise `Process` against `tr` and `sh`, so they need a Unix-like
environment.

## Caveats

- **Arbitrary command execution.** Anyone who can reach the server can register
  and run any command on the host. The server binds to `localhost` for this
  reason. Do not expose it on a network without adding authentication.
- **Registry is in memory.** Registered functions are lost on restart.
- **Timeout does not kill the process.** When an invocation exceeds 2 seconds
  the client receives `504`, but the spawned process keeps running until it
  exits on its own.
- **Registry lock is held during invocation.** Invocations take a read lock on
  the registry, so a registration request will wait until in-flight
  invocations finish.
