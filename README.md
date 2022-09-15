# `camp` (Camelot Academy Math Pace system)

It is the author's hope that this document contains all the information
necessary to deploy this system into production.

## Preparation for Deployment

This system is intended to be deployed as a
[Google Cloud service](https://cloud.google.com/) with two parts:
the server process, to be deployed as a Docker container on
[Google Cloud Run](https://console.cloud.google.com/run), and a backing
[Google Cloud SQL](https://console.cloud.google.com/sql) database store.

### 0. Your System Requirements

The server is written in Rust, and requires
[The Rust Toolchain](https://www.rust-lang.org/tools/install). It is meant to
be deployed in an Alpine Linux container, and thus requires the
`x86_64-unknown-linux-musl` target:

```sh
rustup target install x86_64-unknown-linux-musl
```

But the Rust compiler may still not have everything it needs to build for the
`musl` target, so you may need to install some more stuff. For example, on
Debian, you also need the `must-tools` Debian package installed.

You will also need Docker to build/push your container.
The [Docker Engine](https://docs.docker.com/engine/install/) is fine,
you won't need the whole Docker Desktop.

You may also need the
[`gcloud` command-line tool](https://cloud.google.com/sdk/docs/install)
in order to provide authentication for Docker pushing your server's
container to the [Artifact Registry](https://console.cloud.google.com/artifacts).

### 1. Create the Database

Create a Google Cloud SQL instance with a Postgres 14 instance (other
versions of Postgres will almost assuredly work, but this has been verifiedly
deployed with a version 14 store). Create a user for the sytem to use and
two databases to which the user has access. One of these will be the
authorization database, the other will be the "data" database.

(Creating users and databases can be done through the Cloud SQL console,
but it may be easier to use the Cloud Shell and cnnect to the instance
directly. See
[CREATE ROLE](https://www.postgresql.org/docs/current/sql-createrole.html)
and
[CREATE DATABASE](https://www.postgresql.org/docs/current/sql-createdatabase.html)
in the
[PostgreSQL Documentation](https://www.postgresql.org/docs/current/index.html).)

Choose the region carefully, and also remember it, because that's where you
want to create all your Google Cloud resources for this project.

Remember these values, you'll need them later:

  * the system's Postgres `$user_name`
  * that user's `$user_password`
  * the name of the `$auth_database`
  * the name of the `$data_database`

### 2. Generate an Artifact Registry

Create an instance of a
[Google Cloud Artifact Registry](https://console.cloud.google.com/artifacts)
repository in the same region as your Cloud SQL instance. Take note of
the URI you'll need to use to push it. (There's a way you can copy this
value to the clipboard by clicking on an icon in the control panel.)
So note

  * `$artifact_registry_uri`

### 3. Sign up for [Sendgrid](https://sendgrid.com/)

Jump through all the hoops; you will ultimately need to take note of your
authorization token. It starts with `Bearer ` and is followed by a bunch
of mostly-alphanumeric characters.

So note

  * `$sendgrid_auth_token`

### 4. Build the Server Process

In the local repository, you should just be able to

```sh
cargo build --release --target x86_64-unknown-linux-musl
```

and everything should work. You'll also want to strip the debugging symbols
from your binary (you don't need 'em in the Docker container!).

```sh
strip target/x86_64-unknown-linux-musl/release/camp
```

### 5. Deploy it Once

It won't work yet, but you need to do this in order to get a URI for your
server process so you can configure it properly.

In order to build the Docker container, you need to create a deployment
configuration file, `deploy/config.toml`. For now, the only thing you need
in that file is your `$sendgrid_auth_token`:

```toml
sendgrid_auth_string = "$sendgrid_auth_token"
```

Build the container and tag it with the appropriate destination:

```sh
docker build -t camp -t $artifact_registry_uri/camp
```

Push it:

```sh
docker push $artifact_registry_uri/camp
```

If you encounter an authentication problem here, try
[setting up `gcloud` authentication for Docker](https://cloud.google.com/artifact-registry/docs/docker/authentication).

Create an service in the Cloud Run console.

  * Use the image you just pushed.
  * Choose the same region you've been using.
  * Allocate CPU only during request processing.
  * Set 0 minimum and 1 maximum instances.
  * Allow all traffic.
  * Set the container port to 80.
  * 512 MB ram and 1 vCPU will work, but you can experiment with less.
  * 60 second request timeout is probably more than enough
  * Under "CONNECTIONS", add a Cloud SQL connection, and choose the SQL
    instance you created earlier.

If you know what you're doing, you can undboutedly change some of these
settings if it would suit your use case.

Also at this point take note of the SQL instance identifier. It will
appear in the drop-down bar when you select the instance to connect to.
Note

  * `$sql_instance_id`

The connection to the database instance will appear in the filesystem
of your server's Docker container at the path
`/cloudsql/$sql_instance_id`. You'll need this for configuration later.

Deploy the service; even if it crashes on deployment, you can still select
it in the console to get the service URI. Make note of the

  * `$service_uri`

### 6. Configure It for Real

At this point you should pick a default Administrator uname/password/email
combo. These will go into your config file to guarantee that this user
exists on deployment. You can use this Admin to log in and add other
Admins. So decide upon:

  * `$default_admin_uname`
  * `$default_admin_pwd`
  * `$default_admin_email`

Now re-edit `deploy/config.toml` with the various configuration values you
have gathered/created:

```toml
uri = "$service_uri"
auth_db_connect_string = "host=/cloudsql/$sql_instance_id user=$user_name password='$user_password' dbname=$auth_database"
data_db_connect_string = "host=/cloudsql/$sql_instance_id user=$user_name password='$user_password' dbname=$data_database"
admin_uname = "$default_admin_uname"
admin_password = "$default_admin_pwd"
admin_email = "$default_admin_email"
sendgrid_auth_string = "$sendgrid_auth_token"
host = "0.0.0.0"
port = 80
```

(Although the `port` value shouldn't matter; it should get passed as an
environment variable to the container, and the server process should
read it from the environment.)

### 7. Deploy it for Real

Rebuild and repush the Docker container:

```sh
docker build -t camp -t $artifact_registry_uri/camp
docker push $artifact_registry_uri/camp
```

From the Cloud Run console, select the service and click on "EDIT AND DEPLOY
A NEW REVISION". The only thing that needs to be changed is the container
image, where you should select the latest version. Deploy, and you're done.