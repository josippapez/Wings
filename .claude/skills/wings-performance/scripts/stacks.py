"""lldb callback for who-redraws.sh, and, run as a script, the summary of what it recorded."""
import collections, os, re, sys


def hit(frame, bp_loc, extra, internal_dict):
    names = [f.GetFunctionName() or '?' for f in list(frame.GetThread().frames)[:45]]
    with open(os.environ['WINGS_STACKS'], 'a') as out:
        out.write(f"{bp_loc.GetBreakpoint().GetID()} || {' <- '.join(names)}\n")
    return False


if __name__ == '__main__':
    path, secs = sys.argv[1], float(sys.argv[2])
    counts = collections.Counter()
    for line in open(path) if os.path.exists(path) else []:
        which, stack = line.split(' || ', 1)
        if which != '1':  # App::notify; invalidate_view repeats most of them
            continue
        ours = [re.sub(r'[<>]', '', f)[:110] for f in stack.strip().split(' <- ')[1:]
                if 'wings_native' in f or 'invoke_observer' in f]
        counts[' <- '.join(ours[:5])] += 1
    print(f"{sum(counts.values())} notifies in {secs:.0f} s")
    for stack, n in counts.most_common(10):
        print(f"\n{n / secs:5.1f}/s  {stack}")
