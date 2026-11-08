// A name of one kind where another is expected.

struct Point { x: int64 }
trait Shape {}

func f<T>(s: Shape) { //~ ERROR expected type, found trait `Shape`
    let p = Point //~ ERROR expected value, found struct `Point`
    let t = T //~ ERROR expected value, found type parameter `T`
}
