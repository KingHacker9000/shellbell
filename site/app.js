const platform = document.querySelector('#platform');
const shell = document.querySelector('#shell');
const relayUrl = document.querySelector('#relay-url');

const command = (id, value) => {
  document.querySelector(`#${id}`).textContent = value;
};

function renderCommands() {
  const selectedShell = shell.value;
  const url = relayUrl.value.trim() || 'https://shellbell.example.com';
  const platformNote = platform.value === 'pi'
    ? 'uname -m  # expected: aarch64\n'
    : platform.value === 'wsl'
      ? 'uname -a  # confirm WSL 2\n'
      : '';

  command(
    'install-command',
    `${platformNote}curl -fsSL https://raw.githubusercontent.com/KingHacker9000/shellbell/main/install.sh | sh -s -- --shell ${selectedShell}`,
  );
  command('pair-command', `shellbell pair ${url}`);
  command('open-command', `Open ${url} and register this browser as a receiver.`);
  command(
    'test-command',
    'shellbell doctor\nshellbell ring "Shellbell setup complete" --to all',
  );
}

for (const input of [platform, shell, relayUrl]) {
  input.addEventListener('input', renderCommands);
}

for (const button of document.querySelectorAll('[data-copy]')) {
  button.addEventListener('click', async () => {
    const target = document.querySelector(`#${button.dataset.copy}`);
    await navigator.clipboard.writeText(target.textContent);
    const original = button.textContent;
    button.textContent = 'Copied';
    setTimeout(() => { button.textContent = original; }, 1200);
  });
}

renderCommands();
