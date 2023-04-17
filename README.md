# Sminit

- sminit is a trivial service manager, trying to mimic [zinit](https://github.com/threefoldtech/zinit)

## sminit manager

<!-- - should concurrently handle tracked services
- each worker should handle a service in a separate thread
- each worker should have a communication path with the manager to receive instructions.
- manager should also be able to handle multiple requests concurrently.
- a service is responsible for its lifetime.
- a manager could signal a service, only a service decides what is the appropriate action.
- a delete signal stops the service and deletes it from the set of tracked services and from all dependencies. i.e. the service is no longer present in the service graph.
- an add signal adds the service to the service graph and sends a start signal to the service.
- a start signal tells the service that some external factor wants it to start, the service does the appropriate checks to ensure that its eligible to start.
- a stop signal tells the service to stop, the service stops immediately.
- the service graph must always be in a correct/clean state.
- if a service passed its health check for the first time since it was last started, all dependent non running services receive a start signal.
- the manager has two responsibilities:
  - tracking services.
  - exposing its api to external users.
- two actors should be able to send a signal to a service:
  - the manager
  - another service
- the manager shoulb be the the access point to a service's senders. -->

- sminit manager's main job is to handle services
- an external actor could ask the manager for the following:
  - track a new service based on a service definition
  - start a tracked service (start the process)
  - stop a tracked service (stop the process, but process info is still in memory)
  - delete a tracked service
- the manager should guarantee thread safety, since multiple actors could send requests to the manager at a time
- there should be a server exposing the manager's functionality to external actors
- users could send requests to the server by using a cli