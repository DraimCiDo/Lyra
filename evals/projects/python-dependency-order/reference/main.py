def order(graph):
    pending={node:set(deps) for node,deps in graph.items()}
    for node in list(pending):
        for dep in pending[node]: pending.setdefault(dep,set())
    result=[]
    while pending:
        ready=sorted(node for node,deps in pending.items() if not deps)
        if not ready: raise ValueError('cycle')
        node=ready[0]; result.append(node); del pending[node]
        for deps in pending.values(): deps.discard(node)
    return result
