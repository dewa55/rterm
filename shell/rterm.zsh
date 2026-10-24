# rterm shell integration for zsh: OSC 133 prompt marks and OSC 7 working directory.
# rterm loads this automatically; source it from ~/.zshrc on remote hosts.

[[ -o interactive ]] || return 0
(( ${+__rterm_si_loaded} )) && return 0
typeset -g __rterm_si_loaded=1

__rterm_precmd() {
  local st=$?
  (( ${+__rterm_prompted} )) && print -n "\e]133;D;${st}\a"
  typeset -g __rterm_prompted=1
  # Percent-encode everything outside the unreserved set.
  local LC_ALL=C dir= c i
  for (( i = 1; i <= ${#PWD}; i++ )); do
    c=${PWD[i]}
    if [[ $c == [a-zA-Z0-9/._~-] ]]; then dir+=$c; else dir+=$(printf '%%%02X' "'$c"); fi
  done
  print -n "\e]7;file://${HOST}${dir}\a"
  print -n "\e]133;A\a"
  return $st
}

__rterm_preexec() {
  print -n "\e]133;C\a"
}

autoload -Uz add-zsh-hook
add-zsh-hook precmd __rterm_precmd
add-zsh-hook preexec __rterm_preexec
# Make sure our precmd runs before themes/plugins added later can change $?.
precmd_functions=(__rterm_precmd ${precmd_functions:#__rterm_precmd})
