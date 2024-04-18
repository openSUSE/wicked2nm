# Migrate wicked
This project creates a `migrate-wicked` binary which is able to parse wicked xml
configs and send them to a NetworkManager dbus service.
## Installation
The main way to use the migration is through the container image available at
`registry.opensuse.org/home/jcronenberg/migrate-wicked/containers/opensuse/migrate-wicked-git:latest`.  
For openSUSE a package that includes all the latest changes is available at
https://build.opensuse.org/package/show/home:jcronenberg:migrate-wicked/migrate-wicked-git.
## Usage
The main recommended way to use `migrate-wicked` is with a container.
```bash
podman run -v /etc/sysconfig/network:/etc/sysconfig/network registry.opensuse.org/home/jcronenberg/migrate-wicked/containers/opensuse/migrate-wicked-git:latest
```
This will create `*.nmconnection` files inside `/etc/sysconfig/network/NM-migrated`.  
See also the [Container's README](https://build.opensuse.org/projects/home:jcronenberg:migrate-wicked/packages/migrate-wicked-git-container/files/README?expand=1)
for further infos.

If you want to run the migration on the system itself the required xml config
needs to be generated via wicked and can the be passed to `migrate-wicked`.
```bash
wicked show-config > wicked.xml
migrate-wicked migrate wicked.xml
# See also migrate-wicked --help for further info
```
## Architecture
`migrate-wicked` uses agama as a library to communicate the parsed network state to NetworkManager
but the binary is completely independent of any agama services and can be run standalone.
