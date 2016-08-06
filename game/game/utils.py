def bf_traverse(vertex_fn, origin):
    yield origin
    queue = [origin]
    visited = set()
    while queue:
        cur = queue.pop()
        vert = vertex_fn(cur)
        if vert and vert not in visited:
            yield vert
            visited.add(vert)
            queue.append(vert)
