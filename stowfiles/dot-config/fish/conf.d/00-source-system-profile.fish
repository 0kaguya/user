if status is-login && test -r /etc/profile

    # guard variable is brutal but it works
    if not set -q system_profile_guard
	set -gx system_profile_guard 1

	exec bash -c 'source /etc/profile && exec fish --login'

    else
	set -e system_profile_guard
    end
end
