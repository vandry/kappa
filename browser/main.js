const {ListPodsRequest, ListPodsResponse} = require('codegen/browser_pb.js');
const {KappaBrowserClient} = require('codegen/browser_grpc_web_pb.js');

var u = URL.parse(window.location);
u.host = "api." + u.host;
var api = new KappaBrowserClient(u.toString());

function escape_identity(u) {
    var segment = (u.protocol.replace(/:/, "")) + "." + u.hostname;
    if (u.pathname === "") {
        return segment;
    }
    return segment + "._" + u.pathname.replace(/\/(_+)\//g, "/_$1/").replace(/\./g, "._.").replace(/\//g, ".");
}

class PodList {
    constructor (api) {
        this.api = api;
        this.pods_el = document.getElementById("pods");
        this.status_el = document.getElementById("status");
        this.host_suffix = window.location.host.replace(/^[^.]*\./, "");
        this.base_url = URL.parse(window.location);
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
            var pod = pods[i];
            var metadata = pod.getMetadata();
            var tr = document.createElement("tr");
            var td = document.createElement("td");
            td.textContent = metadata.getNamespace();
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
                var name = prefix + metadata.getName() + "." + metadata.getNamespace() + ".pod." + this.host_suffix;
                var url = new URL(this.base_url);
                url.host = name;
                url.pathname = "";
                url.port = diag_port;
                var a = document.createElement("a");
                a.href = url;
                a.textContent = "Diag";
                td.appendChild(a);
            }
            tr.appendChild(td);
            this.pods_el.appendChild(tr);
        }
    }
}

var pods = new PodList(api);
pods.reload();
