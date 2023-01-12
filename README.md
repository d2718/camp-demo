# `camp-demo`

This is a demonstration version of the
[Camelot Academy Math Pace System](https://github.com/d2718/camp), modified
to be easily-runnable on one's local machine, along with some made-up
sample data. (The original `camp` is meant to run on the Google Cloud
Platform.)

## TL; DR

Maybe:
```bash
$ rustup update
$ rustup target add x86_64-unknown-linux-musl
```

Definitely:
```bash
$ git clone https://github.com/d2718/camp-demo
$ cd camp-demo
$ bash build.sh
#   ...lots of downloading and building
$ docker compose -f camp-docker/compose.yaml up &
#   ...wait until the log messaging stops spewing
$ ./demo_data
```

## Build

Clone the repo and descend:
```bash
$ git clone https://github.com/d2718/camp-demo

$ cd camp-demo
```

This uses [Docker Compose](https://docs.docker.com/compose/), and binds to
ports 8001 and 5432 on the host machine, so be prepared.

I don't know what the absolute minimum version of Rust required is, but it
definitely works using 1.66 (and may very well work with any version of the
2021 edition). You will also need the `musl` target installed if you don't
already:
```bash
$ rustup target add x86_64-unknown-linux-musl
```
You may also need to install some additional stuff; for example, on
Debian-based systems, you'll need the `musl-tools` package. If you're
on Windows, WSL2 makes this all very easy.

Okay, once all your prereqs are satisfied, you should be able to build
the whole thing with the build script:
```bash
$ bash build.sh
```
Cargo will grind for a while, downloading and building, then it will
copy the necessary binaries to build the images into their places in the
`camp-docker` directory, and also put the program to insert sample data
in the current directory.

## Run

```bash
$ docker compose -f camp-docker/compose.yaml up
```

Once the docker compose log messages stop scrolling, you should be able
to point your browser to

`http://localhost:8001/`

and log in as the default administrator. The default administrator's user
name and password are both `admin`.

However! There is no data in the database yet, except for a single user
account (the default admin, `admin`, in as whom you are logged).

### Install Sample Data

You can insert a tranch of sample data by running the `demo_data` program
that has appeared in the root directory of the repository. (You will want
to open another terminal window, because one of the functions of the
system requires being able to see the Docker Compose log output.)

```bash
$ ./demo_data
```

If you are logged in as the default admin in a browser window, you will have
to log in again to see this data. You should be able to just reload the page.

## Use

Logging in as the default admin will allow you to see all the other users'
user names, so you can log in as other roles (boss, teacher, student) and
see what information and functionality is available to them.

### Resetting Passwords

To reset the password of any user (which you'll have to do for anyone apart
from the admin), logging in unsuccessfully will give you the option to reset
your password by clicking on "I forgot my password." In the production version,
this makes a call to [SendGrid](https://sendgrid.com/) to generate a
recovery email, but for this demonstration the call is sent to a mock service
in one of the containers that just prints the email to it standard output, so
you'll have to _look for the email text in the docker compose log output_.