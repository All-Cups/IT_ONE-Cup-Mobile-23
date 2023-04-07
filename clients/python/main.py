import sys
import random
import string
import requests


PIPES = 3


def main():
    server_addr = sys.argv[1] if 1 < len(sys.argv) else "127.0.0.1:8080"
    api_url = f"http://{server_addr}/api"
    token = sys.argv[2] if 2 < len(sys.argv) else ''.join(
        random.choice(string.ascii_lowercase) for _ in range(16))

    def collect():
        pipe = random.randint(1, PIPES)
        response = requests.put(f"{api_url}/pipe/{pipe}",
                                headers={"Authorization": f"Bearer {token}"})
        assert response.status_code == 200
        print("Collect pipe", pipe, "=", response.json())

    def value():
        pipe = random.randint(1, PIPES)
        response = requests.get(f"{api_url}/pipe/{pipe}/value",
                                headers={"Authorization": f"Bearer {token}"})
        assert response.status_code == 200
        print("Pipe", pipe, "=", response.json())

    def modifier():
        pipe = random.randint(1, PIPES)
        type = random.choice(
            ["slow", "double", "min", "shuffle", "reverse"])
        response = requests.post(f"{api_url}/pipe/{pipe}/modifier",
                                 json={"type": type},
                                 headers={"Authorization": f"Bearer {token}"})
        if response.status_code == 200:
            print("Applied modifier", type, "to pipe", pipe)
        elif response.status_code == 422:
            print("Failed to apply modifier")
        else:
            raise Exception(
                f"Unexpected status code {response.status_code}")

    while True:
        random.choice([collect, value, modifier])()


if __name__ == "__main__":
    main()
