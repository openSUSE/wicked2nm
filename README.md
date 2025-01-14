# Wicked to NetworkManager migration
This project creates a `wicked2nm` binary which is able to parse wicked xml
configs and send them to a NetworkManager dbus service.
## Installation
The main way to use the migration is through the container image available at
`registry.opensuse.org/home/jcronenberg/migrate-wicked/containers/opensuse/wicked2nm:latest`.  
For openSUSE a package that includes all the latest changes is available at
https://build.opensuse.org/package/show/home:jcronenberg:migrate-wicked/wicked2nm.
## Usage
The main recommended way to use `wicked2nm` is with a container.
```bash
podman run -v /etc/sysconfig/network:/etc/sysconfig/network registry.opensuse.org/home/jcronenberg/migrate-wicked/containers/opensuse/wicked2nm:latest
```
This will create `*.nmconnection` files inside `/etc/sysconfig/network/NM-migrated`.  
See also the [Container's README](https://build.opensuse.org/projects/home:jcronenberg:migrate-wicked/packages/wicked2nm-container/files/README?expand=1)
for further infos.

If you want to run the migration on the system itself the required xml config
needs to be generated via wicked and can the be passed to `wicked2nm`.
```bash
wicked show-config > wicked.xml
wicked2nm migrate wicked.xml
# See also wicked2nm --help for further info
```
## Architecture
`wicked2nm` uses agama as a library to communicate the parsed network state to NetworkManager
but the binary is completely independent of any agama services and can be run standalone.
