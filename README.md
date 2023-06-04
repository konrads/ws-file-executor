# Websocket file executor

[![build](../../workflows/build/badge.svg)](../../actions/workflows/build.yml)

HTTP server that facilitate file upload and command execution on this uploaded file, with streaming stdout back to the page.

Endpoints provided:

- `/` - serves [index.html](static/index.html)
- `/upload` - uploads a file and `file_path` parameter. Persists uploaded file under path `${upload_dir}/${id}/${file_path}/${file_name}`, where `id` is a random uuid. Keeps track of the mentioned path under `id`. Returns the `id` as `X-File-Id` response header
- `/runCommand` - accepts parameters for command and `id`, runs a command on the file. Command is hardcoded on the web page to either: `cat`, `wc`, `sh` or `bogus-cmd`

```mermaid
sequenceDiagram
    actor       User
    participant page as index.html
    participant upload as /upload<br/>(endpoint)
    participant runCommand as /runCommand<br/>(endpoint)
    participant fs as file in fs<br/>(Service)
    participant registry as entry in registry<br/>(Service)
    participant command as command execution<br/>(Service)

    User->>page: load /
    activate page
    User->>page: set command<br/>file_path<br/>file to upload<br/>click 'Run'

    # /upload request
    rect rgb(240, 255, 255)
        page->>upload: uploads file/file_path
        activate fs
        upload->>fs: persist the file under file_path
        activate registry
        upload->>registry: register full file path
        upload-->>page: issues id
    end

    # /runCommand request
    rect rgb(255, 245, 250)
        page->>runCommand: initiate command execution for id
        runCommand->>registry: get full path for id
        runCommand->>command: spawn the command
        activate command
        runCommand-->>page: ws connect
        command-->>runCommand: stdout:line 1
        runCommand-->>page: ws msg: line 1
        command-->>runCommand: stdout:line 2
        runCommand-->>page: ws msg: line 2
        command-->>runCommand: stdout:line 3
        runCommand-->>page: ws msg: line 3
        command-->>runCommand: command finished
        deactivate command
        runCommand-->>page: ws close
        runCommand->>fs: remove file
        deactivate fs
        runCommand->>registry: deregister file
        deactivate registry
    end
    deactivate page
```

## Assumptions

- upload and command execution are performed in 2 separate requests, as POST `/upload` cannot upgrade to WS connection - this is handled GET `/runCommand` endpoint which takes `id` provided by `/upload`. Orchestration of the 2 requests is responsibility of the initiating web page.
- WS connection is initiated from the client (web page)
- `file_path` is a multiform field that whilst added, is not necessary to the mechanism which ids every upload with unique uuid anyways

## Design

There are 2 layers to the design - web routing layer and `Service` layer. The routing is handled by [main.rs](src/main.rs), which wires in the `Service`, establishes routes and other configurations. Out of the 3 endpoints, `/` just serves static [index.html](/static/index.html), '/upload' pass through the `Service`, `/runCommand` passes through to `Service` together with a channel, where stdout is provided, which the endpoint subsequently forwards inside the WS. Heavy lifting is done inside [service.rs](src/service.rs)' `Service`, which handles file persistence, command execution, registration/deregistration of file ids, synchronization. `Service` is implemented as a trait, allowing for testing via [mockall](https://crates.io/crates/mockall). Implementation of the `Service` - `ProdService` is tested as is, with file interactions and test-only helper functions.

`actix_web` was chosen for the server framework, due to its WS support, performance (faster than `Rocket`), and from initial glance - reduced macro complexity.

Front end was influenced by <https://github.com/actix/examples/tree/master/websockets/echo-actorless>.

## Usage

Start server with:

```sh
cargo run
```

Open <http://localhost:8080>, select command (either `cat` | `wc` | `sh` | `bogus-cmd`), type in file_path, select file to upload, hit `Run`. File is uploaded, registered, WS connection is established, command execution initiated and stdout piped back to the web page via WS, until WS is closed when no more stdout output. Note: for `sh` testing, best to choose a shell script (eg. [roundtrip1.sh](tests/stage/scripts/roundtrip1.sh)) with `sleep`s inside, as this exhibits the nature of the streamed output. `bogus-cmd` is used for testing failure scenarios.

## Testing

For unit testing:

```sh
cargo test
```

With selenium integration tests (for setup checkout [build.yml](.github/workflows/build.yml)):

```sh
pip install selenium
cargo test --features integration-test
```

Unit testing comprises:

- route availability testing. Note: due to complexity of multiform data/WS upgrade, basic availability is tested
- `ProdService`'s upload and cmd execution
- white box parallel testing of upload/cmd execution, mimicking web page interaction. Note: this doesn't go through `actix_web` machinery.

For manual testing, see [Usage](#usage), and play with file imports/command running. For error conditions, feel free to select `bogus-cmd` for command, or [evil-cmd.sh](tests/stage/scripts/evil-cmd.sh)
