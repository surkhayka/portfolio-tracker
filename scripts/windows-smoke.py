"""Exercise the installed app through Windows WebDriver with synthetic data."""
import base64
import json
import os
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path


def request(method, route, body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(
        "http://127.0.0.1:4444" + route, data=data, method=method,
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=45) as response:
            result = json.load(response)["value"]
    except urllib.error.HTTPError as error:
        raise RuntimeError(error.read().decode()) from error
    if isinstance(result, dict) and "error" in result:
        raise RuntimeError(result)
    return result


def wait_for(predicate, description):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.15)
    raise AssertionError(description)


def session(application):
    return request("POST", "/session", {
        "capabilities": {"alwaysMatch": {"tauri:options": {
            "application": str(application),
            "webviewOptions": {
                "userDataFolder": str(Path(os.environ["PORTFOLIO_DATA_DIR"]) / "webview-profile"),
            },
        }}},
    })["sessionId"]


class App:
    def __init__(self, application):
        self.id = session(application)
        self.route = "/session/" + self.id
        wait_for(lambda: self.js("return !!document.querySelector('h1')"), "App did not open")

    def js(self, script, *args):
        return request("POST", self.route + "/execute/sync", {"script": script, "args": args})

    def invoke(self, name):
        result = request("POST", self.route + "/execute/async", {
            "script": """
                const done = arguments[arguments.length - 1];
                window.__TAURI_INTERNALS__.invoke(arguments[0]).then(
                    value => done({value}), error => done({error: String(error)}));
            """,
            "args": [name],
        })
        assert "error" not in result, result
        return result["value"]

    def click(self, text):
        self.js("""
            const button = [...document.querySelectorAll('button')].find(
                element => element.textContent.trim() === arguments[0]);
            if (!button) throw Error('Missing button ' + arguments[0]);
            button.click();
        """, text)

    def input(self, css, value):
        element = request("POST", self.route + "/element", {"using": "css selector", "value": css})
        element_id = element["element-6066-11e4-a52e-4f735466cecf"]
        request("POST", self.route + "/element/" + element_id + "/value", {"text": value})

    def ready(self, selector):
        wait_for(lambda: self.js("return !!document.querySelector(arguments[0])", selector), selector)

    def screenshot(self, name):
        image = request("GET", self.route + "/screenshot")
        Path("windows-artifacts", name).write_bytes(base64.b64decode(image))

    def close(self):
        request("DELETE", self.route)


def main():
    executable = Path(sys.argv[1]).resolve()
    assert executable.is_file(), executable
    Path("windows-artifacts").mkdir(exist_ok=True)
    driver_log = open("windows-artifacts/webdriver.log", "w", encoding="utf-8")
    driver = subprocess.Popen(
        ["tauri-driver", "--native-driver", shutil.which("msedgedriver")],
        stdout=driver_log, stderr=subprocess.STDOUT,
    )
    app = None
    try:
        def driver_ready():
            try:
                request("GET", "/status")
                return True
            except (OSError, RuntimeError):
                return False
        wait_for(driver_ready, "Windows driver did not start")
        app = App(executable)
        assert app.invoke("portfolio")["holdings"] == []
        assert not app.invoke("get_settings")["connected"]
        assert app.js("return document.documentElement.dataset.platform") == "windows"
        assert app.js("return getComputedStyle(document.querySelector('.titlebar')).display") == "none"
        app.click("Imports")
        app.ready("input[type=file]")
        app.input("input[type=file]", str(Path("public/portfolio-example.csv").resolve()))
        app.ready(".review-rows tbody tr")
        wait_for(lambda: app.js("""
            return [...document.querySelectorAll('button')].some(
                button => button.textContent.trim() === 'Import 7 changes' && !button.disabled);
        """), "Import preview unavailable")
        app.click("Import 7 changes")
        wait_for(lambda: not app.js("return !!document.querySelector('[role=dialog]')"), "Import did not finish")
        assert len(app.invoke("portfolio")["transactions"]) == 7
        app.click("Transactions")
        app.ready(".ledger-scroll")
        app.js("""
            [...document.querySelectorAll('tbody tr')].find(row =>
                [...row.querySelectorAll('.badge')].some(badge => badge.textContent === 'Sell') &&
                row.querySelector('strong')?.textContent === 'AAPL'
            ).querySelector('[aria-label="Exclude AAPL transaction"]').click();
        """)
        wait_for(lambda: app.js("""
            return [...document.querySelectorAll('button')].some(
                button => button.textContent.trim() === 'Confirm changes' && !button.disabled);
        """), "Exclusion preview unavailable")
        app.click("Confirm changes")
        wait_for(lambda: not app.js("return !!document.querySelector('[role=dialog]')"), "Exclusion did not finish")
        app.click("Holdings")
        app.ready("table")
        app.js("document.querySelector('.add-menu').open = true")
        app.click("Add existing holding")
        app.ready("[role=dialog]")
        for label, value in [("Ticker", "AMD"), ("Shares", "2"), ("Total cost basis", "100")]:
            selector = app.js("""
                const label = [...document.querySelectorAll('[role=dialog] label')].find(
                    label => label.textContent.startsWith(arguments[0]));
                label.querySelector('input').id = 'smoke-entry';
                return '#smoke-entry';
            """, label)
            app.input(selector, value)
            app.js("document.querySelector('#smoke-entry').removeAttribute('id')")
        app.click("Review changes")
        wait_for(lambda: app.js("""
            return [...document.querySelectorAll('button')].some(
                button => button.textContent.trim() === 'Confirm changes' && !button.disabled);
        """), "Holding preview unavailable")
        app.click("Confirm changes")
        wait_for(lambda: not app.js("return !!document.querySelector('[role=dialog]')"), "Holding did not save")
        app.click("Settings")
        app.ready("select[aria-describedby=appearance-help]")
        app.js("""
            const select = document.querySelector('select[aria-describedby=appearance-help]');
            select.value = 'dark';
            select.dispatchEvent(new Event('change', {bubbles:true}));
        """)
        wait_for(lambda: app.js("return document.documentElement.dataset.theme") == "dark", "Dark mode failed")
        app.click("Holdings")
        app.ready("table")
        app.screenshot("holdings-dark.png")
        app.close()
        app = None
        time.sleep(1)
        app = App(executable)
        result = app.invoke("portfolio")
        holdings = {holding["symbol"]: holding for holding in result["holdings"]}
        assert holdings["AMD"]["quantity"] == "2"
        assert holdings["AMD"]["basis"] == "100"
        assert holdings["AAPL"]["quantity"] == "20"
        assert next(row for row in result["transactions"]
                    if row["event"]["transaction_id"] == "example-sell-1")["excluded"]
        assert app.js("return document.documentElement.dataset.theme") == "dark"
        assert app.js("return document.querySelectorAll('img[src^=\"https://api.elbstream.com\"]').length") == 0
        print("PASS: installed app launched, imported CSV, excluded trade, added holding, switched theme, and preserved state after restart.")
        Path("windows-artifacts/smoke-result.txt").write_text(
            "Passed desktop smoke test on Windows Server 2022 x64 with WebView2.\n"
            "CSV import, exclusion, opening balance, dark theme, restart persistence, logos off.\n",
            encoding="utf-8",
        )
    except Exception:
        if app:
            try:
                app.screenshot("failure.png")
            except Exception:
                pass
        raise
    finally:
        if app:
            app.close()
        driver.terminate()
        driver.wait(timeout=15)
        driver_log.close()


if __name__ == "__main__":
    main()
