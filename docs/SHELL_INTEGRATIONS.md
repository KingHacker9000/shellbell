# Bash, Zsh, Fish, tmux, and execution boundaries

Managed hooks live under `~/.config/shellbell/hooks`. Startup files contain one marked source line. All hooks generate a fresh random session UUID per shell PID, export `SHELLBELL_SESSION_ID` and safe shell type metadata to children, ignore command-text hook arguments, redirect local helper errors, and fail open.

## Arming boundary

`shellbell on` and `shellbell once` arm the current interactive shell session. Monitoring begins with the next command submitted after the arming prompt returns.

Do not paste the arming command and the monitored command as one multiline prompt submission. Shellbell measures prompt-to-prompt execution boundaries and cannot retroactively count a command that began in the same submission that armed the session.

Correct:

```sh
shellbell once --after 5s --idle 5s --to pc
```

Wait for the prompt, then submit:

```sh
sleep 7
```

After `sleep 7` returns, do not submit another command during the idle window. Any new command cancels settling and extends the current activity burst.

## Bash

The Bash hook applies only to interactive shells. It snapshots the existing `DEBUG` and `EXIT` traps in shell memory, invokes their bodies from wrappers, and does not transmit or persist them. The first DEBUG boundary after a prompt emits `command_start`; a guard prevents recursion and subsequent DEBUG callbacks for the same foreground command do not create time.

`PROMPT_COMMAND` is preserved:

- When it is an array, `__shellbell_precmd` is appended once.
- When it is a string, the existing string runs first and the Shellbell function is appended once.

The Shellbell precmd emits `prompt_ready` and opens the next preexec gate. Existing prompt functions/themes and Starship-style `PROMPT_COMMAND` remain intact. The state machine also rejects duplicate start/prompt events defensively.

## Zsh

The Zsh hook uses `autoload -Uz add-zsh-hook` and registers named functions in native `preexec`, `precmd`, and `zshexit` arrays. It removes only duplicate registrations of those exact Shellbell function names before adding them once. Existing Oh My Zsh, Starship, theme, and user hook-array entries remain untouched. Arguments passed by `preexec` are ignored.

## Fish

The Fish hook defines separate functions for `fish_preexec`, `fish_postexec`, and `fish_exit` native events. It never replaces `fish_prompt`. Event arguments are ignored. Postexec means foreground control returned; a background command returns immediately and therefore reaches prompt-ready immediately.

## Nested shells and tmux

The exported shell PID identifies the shell that created the session. A nested interactive shell inherits it, notices it differs from its own PID, and creates a new random session ID. A noninteractive child inherits the parent's session ID, allowing:

```sh
shellbell ring "Task complete"
```

to associate a deliberate manual ring with the parent's active burst.

Each tmux pane normally starts a separate interactive shell and therefore gets a separate ID, label, arm mode, activity burst, and settling timer. Run `shellbell once` or `shellbell on` in every pane that should ring. Renaming one pane's session does not affect another.

## What foreground means

Activity is actual wall-clock time between shell command-start and prompt-ready boundaries. Shellbell does not inspect subprocesses.

```sh
python train.py &
```

returns prompt control immediately, so only that brief foreground interval counts. Later background lifetime is ignored.

An editor, REPL, agent, or other interactive application that retains the terminal foreground has not returned control to the shell prompt. Output silence is not a completion signal and Shellbell will not automatically ring. The application can explicitly call `shellbell ring` when useful.

Idle means the prompt is ready and no new command has begun for `idle_for`. Shellbell deliberately does not intercept Readline, ZLE, or Fish editor keystrokes. A settling deadline can therefore expire while a slowly typed command has not yet been submitted.
