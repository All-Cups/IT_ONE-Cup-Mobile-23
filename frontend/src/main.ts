import { Chart } from 'chart.js/auto';
import './style.css'

let playersGraph = new Chart(document.querySelector<HTMLCanvasElement>("#players-graph")!, {
    type: "line",
    data: {
        datasets: []
    },
    options: {
        scales: {
            x: {
                type: 'linear',
                position: 'bottom'
            }
        },
    }
});

type Modifier = "slow" | "double" | "min" | "shuffle" | "reverse";

type LogMessage =
    | {
        type: "CollectStart",
        user: string,
        pipe_id: number,
        delay: number,
    }
    | {
        type: "UpdatePipe",
        value: number,
        base_delay: number,
        direction: "Up" | "Down",
        modifiers: {
            [mod in Modifier]?: number
        }
    }
    | {
        type: "CollectEnd",
        user: string,
    }
    | {
        type: "UpdateUser",
        user: string,
        score: number,
    };

type LogEntry = {
    time: number,
    msg: LogMessage,
};

function appendLogEntry(data: LogEntry) {
    let time = data.time;
    let msg = data.msg;
    switch (msg.type) {
        case 'UpdateUser':
            let user = playerNames.get(msg.user) ?? msg.user;
            let playerGraph = playersGraph.data.datasets.find((dataset) => dataset.label == user);
            if (!playerGraph) {
                playerGraph = {
                    label: user,
                    data: [],
                    stepped: true,
                };
                playersGraph.data.datasets.push(playerGraph);
                for (let i = 0; i < playersGraph.data.datasets.length; i++) {
                    playersGraph.data.datasets[i].borderColor = `hsl(${i / playersGraph.data.datasets.length}turn 100% 50%)`;
                }
            }
            playerGraph.data.push({ x: time, y: msg.score });
            playersGraph.update();
            break;
    }
}

const params = new URLSearchParams(location.search);

const playerNames = new Map<string, string>();
const clientIds = params.getAll("client-ids");
const clientNames = params.getAll("player-names");
for (let i = 0; i < clientIds.length; i++) {
    playerNames.set(clientIds[i], clientNames[i]);
}

const replayUrl = params.get("replay");
if (replayUrl) {
    async function loadReplay() {
        let resp = await fetch(replayUrl!);
        let text = await resp.text();
        for (let line of text.split('\n')) {
            let json = line.trim();
            if (json.length != 0) {
                appendLogEntry(JSON.parse(json));
            }
        }
    }
    loadReplay();
} else {
    function ws_url(): string {
        const location = window.location;
        const ws_query = params.get("ws");
        if (ws_query) {
            return ws_query;
        }
        let ws_url;
        if (location.protocol == "https") {
            ws_url = "wss://";
        } else {
            ws_url = "ws://";
        }
        ws_url += location.host;
        ws_url += "/logs";
        return ws_url;
    }
    let ws = new WebSocket(ws_url());
    ws.onmessage = (message) => {
        appendLogEntry(JSON.parse(message.data));
    };
}