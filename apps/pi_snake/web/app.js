const board = document.querySelector('#board');
const status = document.querySelector('#status');
async function post(path, data = '') {
  await fetch(path, { method: 'POST', headers: {'Content-Type':'application/x-www-form-urlencoded'}, body: data });
}
async function draw() {
  try {
    const game = await (await fetch('/api/state', {cache:'no-store'})).json();
    board.style.gridTemplateColumns = `repeat(${game.width}, 14px)`;
    const marks = new Map(game.apples.map(([x,y]) => [`${x},${y}`, ['@','apple']]));
    game.players.forEach((player, playerIndex) => player.snake.forEach(([x,y], index) => marks.set(`${x},${y}`, [index ? 'o' : '3', index ? 'body' : `p${playerIndex + 1}`])));
    board.replaceChildren(...Array.from({length: game.width * game.height}, (_, index) => {
      const x = index % game.width, y = Math.floor(index / game.width), mark = marks.get(`${x},${y}`) || ['',''];
      const cell = document.createElement('span'); cell.className = `cell ${mark[1]}`; cell.textContent = mark[0]; return cell;
    }));
    const p2 = game.players[1];
    status.textContent = `${game.status} ${p2.joined ? 'P2 ready.' : 'Press Join P2.'}`;
  } catch (_) { status.textContent = 'Waiting for the Pi Snake Blueprint on this port…'; }
}
document.querySelector('#join').onclick = () => post('/api/join');
document.querySelectorAll('[data-key]').forEach(button => button.onclick = () => post('/api/input', `key=${encodeURIComponent(button.dataset.key)}`));
addEventListener('keydown', event => {
  const key = event.key.length === 1 ? event.key.toLowerCase() : ({ArrowUp:'w',ArrowLeft:'a',ArrowDown:'s',ArrowRight:'d'}[event.key]);
  if (key && ('wasd3.'.includes(key) || /[0-9]/.test(key))) { event.preventDefault(); post('/api/input', `key=${encodeURIComponent(key)}`); }
});
setInterval(draw, 100); draw();
