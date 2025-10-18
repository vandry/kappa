const {ListPodsRequest, ListPodsResponse} = require('codegen/browser_pb.js');
const {KappaBrowserClient} = require('codegen/browser_grpc_web_pb.js');

var u = URL.parse(window.location);
u.host = "api." + u.host;
var api = new KappaBrowserClient(u.toString());

const HOST_SUFFIX = window.location.host.replace(/^[^.]*\./, "");
const BASE_URL = URL.parse(window.location);

var SERIAL = 0;

function escape_identity(u) {
    var segment = (u.protocol.replace(/:/, "")) + "." + u.hostname;
    if (u.pathname === "") {
        return segment;
    }
    return segment + "._" + u.pathname.replace(/\/(_+)\//g, "/_$1/").replace(/\./g, "._.").replace(/\//g, ".");
}

class Namespace {
    constructor (name, container, template) {
        this.name = name;
        var serial = SERIAL++;
        this.shown = (name.match(/^.*-system$/) === null);
        this.pods = [];
        var el = template.cloneNode(true);
        var l = el.getElementsByClassName("namespace_select");
        this.select = (l.length > 0) ? l[0] : null;
        if (l.length > 0) {
            this.select.checked = this.shown;
            this.select.id = "s" + serial;
            this.select.addEventListener("click", e => {
                if (e.target.checked !== this.shown) {
                    this.shown = e.target.checked;
                    container.notify_namespace_shown(this.shown);
                    this.update();
                }
            }, false);
        }
        var l = el.getElementsByClassName("namespace_name");
        if (l.length > 0) {
            l[0].textContent = name;
            if (this.select !== null) {
                l[0].setAttribute("for", this.select.id);
            }
        }
        var l = el.getElementsByClassName("namespace_only");
        if (l.length > 0) {
            l[0].addEventListener("click", e => {
                container.select_only(this);
            }, false);
        }
        this.el = el;
    }

    add_pod = (pod) => {
        this.pods.push(pod);
    }

    showhide = (container, shown) => {
        if (shown !== this.shown) {
            if (this.select !== null) {
                this.select.checked = shown;
            }
            this.shown = shown;
            container.notify_namespace_shown(this.shown);
            this.update();
        }
    }

    update = () => {
        for (var i = 0; i < this.pods.length; i++) {
            this.pods[i].showhide(this.shown);
        }
    }
}

class NamespaceList {
    constructor () {
        this.n_shown = 0;
        this.l = {};
        var template_src = document.getElementById("namespace_template");
        this.namespace_container = template_src.parentNode;
        this.namespace_template = template_src.cloneNode(true);
        this.namespace_template.removeAttribute("id");
        this.namespace_template.removeAttribute("style");
        this.namespace_all = document.getElementById("namespace_all");
        this.namespace_all.addEventListener("click", e => {
            for (let key in this.l) {
                this.l[key].showhide(this, true);
            }
            e.target.checked = true;
        }, false);
        this.namespace_all.checked = true;
    }

    select_only = (namespace) => {
        for (let key in this.l) {
            this.l[key].showhide(this, this.l[key] === namespace);
        }
    }

    get_or_add = (name) => {
        if (name in this.l) {
            return this.l[name];
        }
        var ns = new Namespace(name, this, this.namespace_template);
        var successor = null;
        var successor_el = null;
        for (let key in this.l) {
            if (key < name) continue;
            if ((successor === null) || (key < successor)) {
                successor = key;
                successor_el = this.l[key].el;
            }
        }
        this.l[name] = ns;
        this.namespace_container.insertBefore(ns.el, successor_el);
        if (ns.shown) {
            this.n_shown += 1;
        } else {
            this.namespace_all.checked = false;
        }
        return ns;
    }

    notify_namespace_shown = (shown) => {
        if (shown) {
            this.n_shown += 1;
            if (this.n_shown >= Object.keys(this.l).length) {
                this.namespace_all.checked = true;
            }
        } else {
            this.n_shown -= 1;
            this.namespace_all.checked = false;
        }
    }
}

class Pod {
    constructor (pod_proto, namespaces) {
        var metadata = pod_proto.getMetadata();
        var namespace_name = metadata.getNamespace();
        var ns = namespaces.get_or_add(namespace_name);
        var tr = document.createElement("tr");
        var td = document.createElement("td");
        td.textContent = namespace_name;
        tr.appendChild(td);
        var td = document.createElement("td");
        td.textContent = metadata.getName();
        tr.appendChild(td);
        var annotations = metadata.getAnnotationsList();
        var diag_port = null;
        var identity = null;
        for (var j = 0; j < annotations.length; j++) {
            if (annotations[j].getKey() === "server-diag-port") {
                diag_port = annotations[j].getValue();
            } else if (annotations[j].getKey() === "server-identity") {
                identity = URL.parse(annotations[j].getValue());
            }
        }
        var td = document.createElement("td");
        if (diag_port !== null) {
            var prefix = (identity === null) ? "http." : (escape_identity(identity) + "._.https.");
            var name = prefix + metadata.getName() + "." + metadata.getNamespace() + ".pod." + HOST_SUFFIX;
            var url = new URL(BASE_URL);
            url.host = name;
            url.pathname = "";
            url.port = diag_port;
            var a = document.createElement("a");
            a.href = url;
            a.textContent = "Diag";
            td.appendChild(a);
        }
        if (!ns.shown) {
            tr.style = "display: none;";
        }
        tr.appendChild(td);
        this.el = tr;
        ns.add_pod(this);
    }

    showhide = (shown) => {
        if (shown) {
            this.el.removeAttribute("style");
        } else {
            this.el.style = "display: none;";
        }
    }
}

class PodList {
    constructor (api) {
        this.api = api;
        this.pods_el = document.getElementById("pods");
        this.status_el = document.getElementById("status");
        this.namespaces = new NamespaceList();
    }

    reload = () => {
        this.api.listPods(new ListPodsRequest(), {}, (err, response) => {
            if (err === null) {
                this.status_el.textContent = response.getPodsList().length + " pods";
                this.populate(response);
            } else {
                this.status_el.textContent = err;
                this.pods_el.textContent = "";
            }
        });
    }

    populate = (response) => {
        this.pods_el.textContent = "";
        var pods = response.getPodsList();
        pods.sort((a, b) => {
            var am = a.getMetadata();
            var bm = b.getMetadata();
            if (am.getNamespace() == bm.getNamespace()) {
                if (am.getName() > bm.getName()) {
                    return 1;
                } else {
                    return -1;
                }
            } else if (am.getNamespace() > bm.getNamespace()) {
                return 1;
            } else {
                return -1;
            }
        });
        for (var i = 0; i < pods.length; i++) {
            var pod = new Pod(pods[i], this.namespaces);
            this.pods_el.appendChild(pod.el);
        }
    }
}

var pods = new PodList(api);
pods.reload();
