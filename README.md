# Starter: a runner for multiple commands

In many cases, you need to run multiple commands in parallel. For example, as web developer you usually need to start the backend and the frontend servers.
Instead of going to each folder and running the command, you can use this program that starts all the needed commands based on a configuration file.

This is an example of a configuration file:

```yaml
processes:
  - name: "Ping"
    command: "ping"
    args: ["-c", "10", "google.com"]
    cwd: "."

  - name: "List"
    command: "ls"
    args: ["-l", "."]
    cwd: "."
```

Once you have this file, assuming it is named `runner.yaml`, you can run the following command to start all the processes:

```bash
starter runner.yaml
```
