"""Local-only prototype: python src/ui-layout-prototype/serve.py."""
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import argparse

parser = argparse.ArgumentParser()
parser.add_argument("--port", type=int, default=8876)
parser.add_argument("--ready-file", type=Path)
options = parser.parse_args()
directory = Path(__file__).resolve().parent
server = ThreadingHTTPServer(("127.0.0.1", options.port), partial(SimpleHTTPRequestHandler, directory=str(directory)))
url = f"http://127.0.0.1:{server.server_port}/?variant=A"
if options.ready_file:
    options.ready_file.write_text(url, encoding="utf-8")
print(url, flush=True)
server.serve_forever()
