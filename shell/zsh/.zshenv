# rterm points ZDOTDIR here so this file runs first. Restore the user's ZDOTDIR, run
# their .zshenv, then load shell integration. .zprofile/.zshrc are read from the
# restored ZDOTDIR as usual.
if [[ -n ${RTERM_ORIG_ZDOTDIR+x} ]]; then
  ZDOTDIR=$RTERM_ORIG_ZDOTDIR
  unset RTERM_ORIG_ZDOTDIR
else
  unset ZDOTDIR
fi
[[ -f ${ZDOTDIR:-$HOME}/.zshenv ]] && source "${ZDOTDIR:-$HOME}/.zshenv"
if [[ -o interactive && -n ${RTERM_SHELL_INTEGRATION_DIR-} ]]; then
  source "$RTERM_SHELL_INTEGRATION_DIR/rterm.zsh"
fi
