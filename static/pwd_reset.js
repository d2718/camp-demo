const DISPLAY = {
    message: document.getElementById("login-error"),
    link: document.getElementById("forgot-link"),
    uname: document.getElementById("uname"),
    instructions: document.getElementById("forgot-instructions"),
    form: document.forms["forgot"],
    button: document.getElementById("forgot-submit"),
    err_div: document.getElementById("error"),
    err_list: document.querySelector("div#error > ul"),
    err_dismiss: document.getElementById("dismiss-errors"),
    prog_div: document.getElementById("progress"),
    prog_list: document.querySelector("div#progress > ul"),
};

function clear(elt) {
    while(elt.firstChild) {
        clear(elt.lastChild);
        elt.removeChild(elt.lastChild);
    }
}

function add_err(err) {
    const item = document.createElement("li");
    item.appendChild(document.createTextNode(err));
    DISPLAY.err_list.appendChild(item);
    DISPLAY.err_div.style.display = "flex";
}
DISPLAY.err_dismiss.addEventListener("click", () => {
    DISPLAY.err_div.style.display = "none";
    clear(DISPLAY.err_list);
})

function make_request(req, desc, on_success) {
    const item = document.createElement("li");
    item.appendChild(document.createTextNode(desc));
    DISPLAY.prog_list.appendChild(item);
    DISPLAY.prog_div.style.display = "flex";

    fetch(req)
    .then(r => {
        if(!r.ok) {
            r.text()
            .then(t => {
                add_err(`Request Error: ${t} (${r.status}: ${r.statusText})`);
            }).catch(e => {
                add_err(`Page Error: ${e}`);
            });
        } else {
            on_success(r);
        }
    })
    .catch(add_err)
    .finally(() => {
        DISPLAY.prog_list.removeChild(item);
        if(!DISPLAY.firstChild) {
            DISPLAY.prog_div.style.display = "none";
        }
    });
}

function show_form(r) {
    DISPLAY.instructions.style.display = "block";
    DISPLAY.form.style.display = "grid";
}

function request_auth_key() {
    const uname = DISPLAY.uname.value.trim();
    if(!uname) {
        RQ.add_err("You must enter a valid user name to reset your password.");
        return;
    }

    const opts = {
        method: "GET",
        headers: {
            "x-camp-uname": uname,
            "x-camp-action": "request-email"
        },
    };

    const r = new Request("/pwd", opts);

    make_request(r, "Sending email with key...", show_form);
}

DISPLAY.link.addEventListener("click", request_auth_key);

function suggest_login(r) {
    DISPLAY.instructions.style.display = "none";
    DISPLAY.form.style.display = "none";

    const msg = "Your password has been reset. You may use the form below to log in with your new password.";

    clear(DISPLAY.message);
    DISPLAY.message.appendChild(document.createTextNode(msg));
}

function reset_password(evt) {
    evt.preventDefault();

    const form = DISPLAY.form;
    const data = new FormData(form);
    const uname = DISPLAY.uname.value.trim();
    const key = data.get("key").trim();
    const password = data.get("password");

    if(!uname) {
        RQ.add_err("You must enter your user name to reset your password.");
        return;
    }
    if(!key) {
        RQ.add_err("Please copy and paste the key from the email your received into the \"key\" field.");
        return;
    }

    const opts = {
        method: "GET",
        headers: {
            "x-camp-uname": uname,
            "x-camp-key": key,
            "x-camp-password": password,
            "x-camp-action": "reset-password"
        }
    };

    const r = new Request("/pwd", opts);

    make_request(r, "Updating passwrod...", suggest_login);
}

document.getElementById("forgot-submit")
    .addEventListener("click", reset_password);