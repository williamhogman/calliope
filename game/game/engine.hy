(import [cocos :as c]
        [numpy :as np]
        [game.geo :as geo]
        [game.color :as color]
        pyglet
        [scipy.misc [toimage]]
        PIL)

(defclass DiagLayer [c.layer.Layer]
  (defn --init-- [self]
    (.--init-- (super DiagLayer self))
    (let [label (c.text.Label "Hello World")]
      (setv label.position (, 320 240))
      (.add self label))))

(defclass MapLayer [c.layer.Layer])

(defn start []
  (c.director.director.init)
  (let [diag-layer (DiagLayer)
        map-layer (create-map-layer )
        main-scene (c.scene.Scene map-layer diag-layer)]
    (c.director.director.run main-scene)))

(defn np->texture [npa]
  (let [[_ x y] npa.shape]
    (pyglet.image.ImageData x y "RGB" (.tobytes (toimage npa)))))

(defn generate-world-sprite []
  (let [w (geo.generate-layers)
        pixs (np->texture (np.array (color.world->pixels w)))]
    (c.sprite.Sprite pixs)))


(defn create-map-layer []
  (let [s (generate-world-sprite)
        ml (MapLayer)]
    (.add ml s)))
