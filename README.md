# MLess

## Components

MLess conists of three primary components.

- Deployment
- Server Daemon
- App Instance
- Scheduler

### Server Daemon

Server Daemon runs every Server.  
It is responsible for proxying incoming request to the approriate App Instance.

It is implemented in Go, using [grpc-proxy](https://github.com/mwitkow/grpc-proxy), to reduce implementation effort.