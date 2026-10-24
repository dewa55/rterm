# rterm shell integration for bash: OSC 133 prompt marks and OSC 7 working directory.
# rterm loads this automatically. To use it on a remote host, copy it there and source it
# from ~/.bashrc. The escape codes match iTerm2/FinalTerm, so other terminals ignore or reuse them.

[[ $- == *i* ]] || return 0
[[ -n ${__rterm_si_loaded-} ]] && return 0
__rterm_si_loaded=1

# Percent-encodes $1 into REPLY (no subshell on every prompt).
__rterm_urlencode() {
  local LC_ALL=C s=$1 c i
  REPLY=
  for (( i = 0; i < ${#s}; i++ )); do
    c=${s:i:1}
    case $c in
      [a-zA-Z0-9/._~-]) REPLY+=$c ;;
      *) printf -v c '%%%02X' "'$c"; REPLY+=$c ;;
    esac
  done
}

__rterm_precmd() {
  local status=$?
  if [[ -n ${__rterm_prompted-} ]]; then
    printf '\e]133;D;%s\a' "$status"
  fi
  __rterm_prompted=1
  __rterm_urlencode "$PWD"
  printf '\e]7;file://%s%s\a' "${HOSTNAME-}" "$REPLY"
  printf '\e]133;A\a'
  # Hand the original status to the rest of PROMPT_COMMAND.
  return "$status"
}

# Run first so $? is still the command's exit status.
if [[ $(declare -p PROMPT_COMMAND 2>/dev/null) == "declare -a"* ]]; then
  PROMPT_COMMAND=(__rterm_precmd "${PROMPT_COMMAND[@]}")
else
  PROMPT_COMMAND="__rterm_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
fi

# PS0 is printed after a command line is read, right before it runs.
PS0+=$'\e]133;C\a'
