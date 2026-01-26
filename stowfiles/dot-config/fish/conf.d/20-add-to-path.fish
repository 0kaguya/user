function add_to_path
    test -d "$argv[1]" && fish_add_path -g "$argv[1]"
end

add_to_path "$HOME/.local/bin"
